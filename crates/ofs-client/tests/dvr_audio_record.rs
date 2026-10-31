//! A recording logs the audio inputs beside the AVI: the client's `dvr_*` calls against a real (in-process) server.
mod common;

use std::path::{Path, PathBuf};

use common::*;
use ofs_client::dvr::DvrFeed;
use ofs_client::{Client, EventKind, Phase, Settings, Sticks, Update};

fn fresh_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ofs-dvr-record-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn sidecar(avi: &Path) -> Vec<String> {
    std::fs::read_to_string(avi.with_extension("audio")).unwrap().lines().map(str::to_string).collect()
}

/// A client whose server never answers: no telemetry, ever.
fn unconnected() -> Client {
    Client::start(Settings { server_addr: "127.0.0.1:1".into(), ..Settings::new(QUAD) }).unwrap()
}

fn start(client: &Client, feed: DvrFeed, avi: &Path) -> Result<(), String> {
    client.dvr_start(feed, avi.display().to_string(), 4, 2)
}

#[test]
fn frames_before_any_telemetry_are_silent_records() {
    let dir = fresh_dir("silent");
    let avi = dir.join("rec.avi");
    let client = unconnected();
    start(&client, DvrFeed::Goggles, &avi).unwrap();
    for _ in 0..3 {
        client.dvr_write_frame(DvrFeed::Goggles, vec![0xFF; 8]);
    }
    assert_eq!(client.dvr_stop(DvrFeed::Goggles).unwrap().frames, 3);
    assert_eq!(sidecar(&avi), ["ofs-dvr-audio 1", "F NaN 0 0 0 0", "F NaN 0 0 0 0", "F NaN 0 0 0 0"]);
}

#[test]
fn frames_carry_the_newest_telemetry_and_a_collision_precedes_the_next_frame() {
    let dir = fresh_dir("live");
    let avi = dir.join("rec.avi");
    let server = TestServer::start();
    let mut probe = Probe::start(server.settings());
    probe.wait_phase(Phase::Flying, LONG);
    probe.client.set_sticks(Sticks { throttle: 0.9, aux: [1.0, -1.0, -1.0, -1.0], ..Default::default() });
    probe.wait("the drone climbs", LONG, |p| p.client.telemetry().is_some_and(|t| t.altitude_m > 1.0));
    start(&probe.client, DvrFeed::Goggles, &avi).unwrap();
    probe.client.dvr_write_frame(DvrFeed::Goggles, vec![0xFF; 8]);
    // Fall back down: the landing is a collision event, which the next poll logs.
    probe.client.set_sticks(Sticks { aux: [1.0, -1.0, -1.0, -1.0], ..Default::default() });
    probe.wait("a collision", LONG, |p| p.event_kinds().contains(&EventKind::Collision));
    let time_s = probe.client.telemetry().unwrap().time_s;
    probe.client.dvr_write_frame(DvrFeed::Goggles, vec![0xFF; 8]);
    probe.client.dvr_stop(DvrFeed::Goggles).unwrap();

    let lines = sidecar(&avi);
    assert_eq!(lines.len(), 4, "{lines:?}");
    assert!(lines[1].starts_with("F ") && !lines[1].starts_with("F NaN"), "{lines:?}");
    assert!(lines[2].starts_with("C "), "{lines:?}");
    let event_time = probe.log.iter().find_map(|u| match u {
        Update::Event(e) if e.kind == EventKind::Collision => Some(e.time_s),
        _ => None,
    });
    assert_eq!(lines[2], format!("C {}", event_time.unwrap()));
    let state_time: f64 = lines[3].split(' ').nth(1).unwrap().parse().unwrap();
    assert!(state_time >= time_s - 0.5 && state_time <= time_s + 1.0, "{} vs {time_s}", lines[3]);
}

#[test]
fn a_refused_start_creates_nothing_and_a_stale_sidecar_does_not_block_one() {
    let dir = fresh_dir("start");
    let client = unconnected();

    let taken = dir.join("taken.avi");
    std::fs::write(&taken, b"earlier recording").unwrap();
    assert!(start(&client, DvrFeed::Goggles, &taken).unwrap_err().contains("already exists"));
    assert!(!taken.with_extension("audio").exists());
    assert_eq!(std::fs::read(&taken).unwrap(), b"earlier recording");
    assert!(client.dvr_stop(DvrFeed::Goggles).is_err(), "nothing was left recording");

    let stale = dir.join("stale.avi");
    std::fs::write(stale.with_extension("audio"), "leftover from a recording whose AVI is gone\n").unwrap();
    start(&client, DvrFeed::Goggles, &stale).unwrap();
    client.dvr_stop(DvrFeed::Goggles).unwrap();
    assert_eq!(sidecar(&stale), ["ofs-dvr-audio 1"]);
}

#[test]
fn a_sidecar_that_cannot_be_created_removes_the_avi() {
    let dir = fresh_dir("nosidecar");
    let client = unconnected();
    let avi = dir.join("rec.avi");
    std::fs::create_dir(avi.with_extension("audio")).unwrap(); // a directory in the sidecar's place
    let err = start(&client, DvrFeed::Goggles, &avi).unwrap_err();
    assert!(err.contains("rec.audio"), "{err}");
    assert!(!avi.exists(), "the AVI of the refused start is removed");
    assert!(client.dvr_stop(DvrFeed::Goggles).is_err());
    // The slot is free again.
    std::fs::remove_dir(avi.with_extension("audio")).unwrap();
    start(&client, DvrFeed::Goggles, &avi).unwrap();
    client.dvr_stop(DvrFeed::Goggles).unwrap();
}
