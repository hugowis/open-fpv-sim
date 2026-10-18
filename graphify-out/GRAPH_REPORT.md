# Graph Report - open-fpv-sim  (2026-10-12)

## Corpus Check
- 209 files · ~372,326 words
- Verdict: corpus is large enough that graph structure adds value.

## Summary
- 2984 nodes · 6227 edges · 184 communities (134 shown, 36 thin omitted)
- Extraction: 95% EXTRACTED · 5% INFERRED · 0% AMBIGUOUS · INFERRED: 295 edges (avg confidence: 0.85)
- Token cost: 1,352,195 input · 15,500 output

## Community Hubs (Navigation)
- Config Loading & Quad Files
- SITL Process Launch
- MSP OSD Client
- Rigid Body Physics
- ELRS Scheduler & Frames
- SimService gRPC Surface
- FPV Interpolation Buffer
- Session & Events
- Godot Client Bridge
- Client Data Model
- Quad Config Model
- M3c Collision & ELRS Docs
- DVR Writer
- Signal Bus Core
- IMU & Vibration Sensors
- RF Propagation
- gRPC Client Streaming
- VTX Paths & Interference
- Pilot Video Streams
- Python Client Tests
- SimServicer RPC Handlers
- Vehicle Assembly
- Python Sim Client
- World Config Parsing
- World Text Writer
- SITL Codec & FDM Packets
- Video Link Stepper
- Python Client Facade
- Grpc
- M4B V1 Fault Catalog (Seven 
- Vtxmodel
- Model
- Server
- Errors
- Stepctx
- Open Fpv Sim - Architecture 
- Crsf
- Faults
- Sitlbridge
- Part
- Vtx
- Worker
- .Drop()
- Pacer
- Videolink Model (Ofs-Video::
- M3A: Osd And Vtx Control Imp
- Collision
- M2B — The Godot Pilot Client
- Msp
- Test Sitl Video
- M3D: Flight-Controller Telem
- Propeller
- Grpc Sim Service (Proto/Ofs/
- Handles
- Vehicle
- Ofs Spike
- The Analog Video Link Model 
- Elrslink
- M2A - Radio Link, Real-Time 
- Shared
- Session
- Dvec3
- Simerror
- Sitl Errors
- The Component Library'S Prov
- Sitlbridge Lockstep Model (F
- Sim
- Wire
- Open Fpv Sim (The Project)
- Test Sitl Link
- Serverprocess
- Radioantennasection
- Names
- Osd
- Shared
- Model
- T A5 Crsf Telemetry
- .Validate()
- Bridge
- Sitl Resend
- Client
- Part
- Frames
- Still Quad()
- Ofs-Sim
- M3A Carried Debt
- Broadcast()
- Shape
- Body Shadow Db()
- Baro
- .Push()
- T6 Windows
- Fnv1A64()
- Battery
- Escmotor
- Listen Policy
- Patch Features
- Clienterror
- Probe
- .Push()
- Test Sitl Telemetry
- Test Sitl Tramp
- Frames
- Launch
- Openloopfc
- Kiss Frame()
- Fctelemetryhandle
- Main
- Runner
- Smartaudio
- M4A Component Library (Libra
- T4 Lockstep
- T B2 Erpm Block
- Esctelemetry
- Client Launch
- Throwaway M0 Spike Harness (
- Milestone M4 (Sandbox)
- Osd
- Test Realtime
- Flight
- Next Kinds()
- Osd Font.Png Glyph Atlas (Tw
- Tramp Codec (Crates/Ofs-Vide
- Erpm Datagram Block (Index 2
- Msp
- T3 Conventions
- T5 Features
- Check Table()
- Main()
- Firmware Dir()
- Osd Font Atlas (Osd Font
- T B Dshot Telemetry
- Make Osd Font
- Restartbudget
- .New()
- [Collision] Config (6 Sphere
- Hover
- Serve Realtime
- Patch Ext Time
- .Subscribe()
- Run-Godot-Tests
- Add Erpm Block
- Add Fc Telemetry
- Add Serial In Datagram
- Add Serial Out Datagram
- Deterministic Boot
- Per-Quad Firmware State Dire
- Contact Resolution (Collect,
- Field Drops In The Marginal 
- Library Root Resolution (Lib
- Seven Shipped Opendrone Libr
- Dedicated Fault Rng Stream (
- Battery As Kiss Esc Telemetr
- Event Kind Collision (1 M/S 
- Build-Sitl
- Patch Debug Race
- Apply Spike B
- Ofs-Radio Crate (Elrs/Crsf R
- Crsf Protocol Mappings (Rf M
- Elrs Link Budget (43.6 Km At
- Sitl Fixed Ports (Udp 9001-9
- Interference And Adjacent-Ch
- Video Link Budget (25 Mw Cle
- Cc-By-Sa-4.0 License For Com
- Openfc Lite / Lite Mini Flig
- Openframe 3F/5F Frames
- Openmotor Motors
- Pypi Packaging (Open-Fpv-Sim
- Osd Font License Notice
- Ofs

## God Nodes (most connected - your core abstractions)
1. `DVec3` - 85 edges
2. `Bus` - 72 edges
3. `SimError` - 44 edges
4. `load()` - 44 edges
5. `Scheduler` - 38 edges
6. `QuadConfig` - 36 edges
7. `write_quad()` - 36 edges
8. `Client` - 35 edges
9. `quad_text()` - 35 edges
10. `SimService` - 35 edges

## Surprising Connections (you probably didn't know these)
- `Component library (schema 5 part files with provenance)` --references--> `Provenance requirement for part parameters`  [INFERRED]
  README.md → docs/research/component-library.md
- `Geometric ExpressLRS link (RSSI/SNR/LQ from world geometry)` --references--> `The ExpressLRS link model (docs/research/elrs-link.md)`  [EXTRACTED]
  README.md → docs/research/elrs-link.md
- `M3b: Analog Video Link and Degradation Implementation Plan (2026-10-09)` --references--> `worlds/flat.toml (shipped world: launch pad, gates, buildings, pilot spot, parked quad VTX)`  [EXTRACTED]
  docs/superpowers/plans/2026-10-09-m3b-video-link.md → README.md
- `the_shipped_quad_describes_its_vtx_antenna()` --calls--> `load()`  [INFERRED]
  crates/ofs-config/tests/world.rs → crates/ofs-electrical/tests/battery.rs
- `a_payload_longer_than_msp_v1_allows_is_an_error_not_a_panic()` --calls--> `encode_request()`  [INFERRED]
  crates/ofs-fc/tests/msp.rs → crates/ofs-fc/src/msp.rs

## Import Cycles
- None detected.

## Hyperedges (group relationships)
- **Simulation model pipeline: models implementing the Model trait over the shared signal bus** — docs_superpowers_plans_2026_10_04_m1_headless_core_model_trait, docs_superpowers_plans_2026_10_04_m1_headless_core_signal_bus, docs_superpowers_plans_2026_10_04_m1_headless_core_scheduler, docs_superpowers_plans_2026_10_04_m1_headless_core_rigidbody, docs_superpowers_plans_2026_10_04_m1_headless_core_propeller_model, docs_superpowers_plans_2026_10_04_m1_headless_core_battery_model, docs_superpowers_plans_2026_10_04_m1_headless_core_escmotor, docs_superpowers_plans_2026_10_04_m1_headless_core_imu_model, docs_superpowers_plans_2026_10_04_m1_headless_core_baro_model, docs_superpowers_plans_2026_10_04_m1_headless_core_openloopfc, docs_superpowers_plans_2026_10_04_m1_headless_core_sitl_bridge [EXTRACTED 1.00]
- **Lockstep exchange path with Betaflight SITL (datagram, patch, ports)** — docs_superpowers_plans_2026_10_04_m1_headless_core_sitl_bridge, docs_superpowers_plans_2026_10_04_m1_headless_core_sitl_codec [EXTRACTED 1.00]
- **Pilot input radio path to Betaflight (ELRS link, CRSF bytes, wire, datagram, failsafe)** — docs_superpowers_plans_2026_10_05_m2a_radio_realtime_elrs_link_model, docs_superpowers_plans_2026_10_05_m2a_radio_realtime_crsf_codec, docs_superpowers_plans_2026_10_05_m2a_radio_realtime_wire, docs_superpowers_plans_2026_10_05_m2a_radio_realtime_radio_signal_names, docs_research_sitl_interface_failsafe_timing [EXTRACTED 1.00]
- **Lockstep determinism pipeline (patched SITL, combined datagram, UART-in-datagram, deterministic boot, reply capture)** — docs_research_sitl_interface_lockstep_patch, docs_research_sitl_interface_combined_datagram, docs_research_sitl_interface_uart_in_datagram, docs_research_sitl_interface_deterministic_boot, docs_research_sitl_interface_reply_datagram, scripts_build_sitl_sh [INFERRED 0.90]
- **Shared RF propagation extraction (video-link physics moved unchanged into ofs-rf for the ELRS link)** — docs_research_video_link_propagation_model, docs_research_elrs_link_shared_rf_physics, crates_ofs_rf, crates_ofs_video_src_propagation_rs, crates_ofs_radio_src_elrs_rs [EXTRACTED 1.00]
- **FPV picture pipeline (DisplayPort OSD grid, SNR degradation, glass-to-glass latency, field drops, DVR)** — docs_dev_setup_msp_displayport_osd, docs_research_video_link_picture_thresholds, docs_research_video_link_latency, docs_research_video_link_field_drops, readme_dvr, godot_ui_video_gd [INFERRED 0.85]
- **M4 fault injection pipeline (python dataclass -> proto oneof -> vehicle enum -> fault.* bus signals -> model seams)** — docs_superpowers_plans_2026_10_11_m4b_fault_catalog_python_faults, docs_superpowers_plans_2026_10_11_m4b_fault_catalog_protocol_8, docs_superpowers_plans_2026_10_11_m4b_fault_catalog_fault_enum, docs_superpowers_plans_2026_10_11_m4b_fault_catalog_fault_seams, docs_superpowers_specs_2026_10_11_m4_sandbox_design_fault_catalog [EXTRACTED 1.00]
- **Analog video end-to-end pipeline (propagation -> link model -> field drops -> delayed composite -> SubViewport -> degradation shader)** — docs_superpowers_specs_2026_10_09_m3b_video_link_design_propagation, docs_superpowers_specs_2026_10_09_m3b_video_link_design_video_link_model, docs_superpowers_specs_2026_10_10_m3e_video_extras_design_field_drops, docs_superpowers_plans_2026_10_11_m3e_video_extras_delayed_composite, docs_superpowers_plans_2026_10_11_m3e_video_extras_subviewport_fpv_composite, docs_superpowers_specs_2026_10_09_m3b_video_link_design_degradation_shader [INFERRED 0.85]
- **Component library data flow (part files -> provenance walker -> library-root resolution -> per-motor vectors -> vibration model input)** — library_readme_part_files, docs_superpowers_plans_2026_10_11_m4a_component_library_provenance_walker, docs_superpowers_plans_2026_10_11_m4a_component_library_library_root, docs_superpowers_plans_2026_10_11_m4a_component_library_per_motor_vectors, docs_superpowers_plans_2026_10_11_m4b_fault_catalog_vibration_model [INFERRED 0.85]

## Communities (184 total, 36 thin omitted)

### Community 0 - "Config Loading & Quad Files"
Cohesion: 0.07
Nodes (80): a_collision_section_is_honoured(), a_diff_that_enables_smartaudio_needs_a_vtx_section(), a_diff_that_enables_the_esc_sensor_needs_an_esc_telemetry_section_on_that_uart(), a_diff_that_sends_the_osd_over_msp_needs_an_osd_section_on_that_uart(), a_diffs_motor_poles_must_match_the_motors(), a_missing_kind_directory_names_the_library_root(), a_missing_vtx_part_is_reported_on_the_vtx_section(), a_motor_part_without_pole_pairs_is_a_parse_error() (+72 more)

### Community 1 - "SITL Process Launch"
Cohesion: 0.05
Nodes (65): FcError, launch_hint(), Error, String, startup_hint(), a_hung_wsl_command_times_out(), a_quick_command_returns_its_output(), default_cleanup() (+57 more)

### Community 2 - "MSP OSD Client"
Cohesion: 0.06
Nodes (43): api_version(), armed(), checksum(), encode_request(), MspClient, MspError, MspParser, MspReply (+35 more)

### Community 3 - "Rigid Body Physics"
Cohesion: 0.08
Nodes (50): Aabb, apply(), collect(), CollisionParams, Contact, release_everything(), resolve(), Default (+42 more)

### Community 4 - "ELRS Scheduler & Frames"
Cohesion: 0.08
Nodes (49): Box, Result, Self, Vec, Scheduler, get(), duty_follows_command_with_first_order_lag(), get() (+41 more)

### Community 5 - "SimService gRPC Surface"
Cohesion: 0.12
Nodes (36): error(), invalid_state(), kind_of(), loaded(), parse_sticks(), Code, Into, JoinHandle (+28 more)

### Community 6 - "FPV Interpolation Buffer"
Cohesion: 0.07
Nodes (30): field_floor(), FieldUniforms, FpvBuffer, FpvView, Option, OsdFrame, VecDeque, VideoSync (+22 more)

### Community 7 - "Session & Events"
Cohesion: 0.08
Nodes (39): Any, FcKind, a_new_collision_count_raises_one_collision_event_naming_the_object(), antenna_msg(), event(), faults_activating_and_clearing_become_one_event_each(), losing_and_regaining_video_sync_become_events(), osd_frame_msg() (+31 more)

### Community 8 - "Godot Client Bridge"
Cohesion: 0.07
Nodes (21): Base, number_key(), OfsClient, OfsExtension, osd_dict(), parse_settings(), Option, OsdFrame (+13 more)

### Community 9 - "Client Data Model"
Cohesion: 0.08
Nodes (30): aim_ned(), Event, EventKind, FcTelemetry, LaunchSpec, ned(), OsdFrame, OverrunPolicy (+22 more)

### Community 10 - "Quad Config Model"
Cohesion: 0.09
Nodes (29): BaroSection, BatterySection, BatteryWiring, CollisionSection, default_friction_coeff(), default_restitution(), default_vtx_band(), diff_setting() (+21 more)

### Community 11 - "M3c Collision & ELRS Docs"
Cohesion: 0.08
Nodes (43): ofs_core::shape (crates/ofs-core/src/shape.rs: signed distances, normals, bounds), ofs-physics crate (rigid body, contacts, collision), crates/ofs-physics/src/collision.rs (impulses and events), crates/ofs-physics/src/rigid_body.rs (contact half, landing points), crates/ofs-radio/src/elrs.rs (the ELRS link model on the bus), ofs-rf crate (propagation, fading, diversity, body shadow shared by video and radio), crates/ofs-rf/src/fading.rs (Fader, Diversity), crates/ofs-rf/src/propagation.rs (path_gain, shared RF physics) (+35 more)

### Community 12 - "DVR Writer"
Cohesion: 0.10
Nodes (30): DvrSummary, DvrWriter, Header, patch_sizes(), put(), put16(), put32(), Arc (+22 more)

### Community 13 - "Signal Bus Core"
Cohesion: 0.11
Nodes (21): Clone, Copy, Bus, BusValue, DQuat, f64, BTreeMap, Default (+13 more)

### Community 14 - "IMU & Vibration Sensors"
Cohesion: 0.10
Nodes (29): Imu, ImuParams, ChaCha8Rng, Normal, Option, Result, Self, Vec (+21 more)

### Community 15 - "RF Propagation"
Cohesion: 0.13
Nodes (34): adjacent_channel_rejection_db(), dbm_to_mw(), deepest_point(), fresnel_v(), knife_edge_loss_db(), mw_to_dbm(), Obstacle, obstruction_db() (+26 more)

### Community 16 - "gRPC Client Streaming"
Cohesion: 0.08
Nodes (18): Client, lock_receiver(), Arc, Drop, Mutex, MutexGuard, Option, OsdFrame (+10 more)

### Community 17 - "VTX Paths & Interference"
Cohesion: 0.18
Nodes (29): fspl_db(), snr_db(), vtx_paths(), a_building_between_the_quad_and_the_pilot_costs_its_loss(), a_hovering_quad_sees_a_steady_signal_and_a_moving_one_a_fading_one(), a_relock_needs_five_good_fields_in_a_row(), an_emitter_interferes_less_the_further_its_channel_is(), bare() (+21 more)

### Community 18 - "Pilot Video Streams"
Cohesion: 0.13
Nodes (31): not_loaded(), feed(), off_runtime(), osd_feed(), osd_rate(), pilot(), PilotGuard, PilotLink (+23 more)

### Community 19 - "Python Client Tests"
Cohesion: 0.06
Nodes (8): AntennaRssi, Received signal in dBm by goggle antenna name, in the world file's order. A…, RadioLoss, Extra per-packet uplink loss while active: `fraction` of packets are dropped on…, test_a_radio_loss_fault_round_trips_through_the_generated_stub(), test_equal_rssi_mappings_hash_alike_whatever_their_order(), test_states_are_hashable_values(), test_video_messages_convert()

### Community 20 - "SimServicer RPC Handlers"
Cohesion: 0.06
Nodes (18): Simulator control, protocol version 8: protocol 7 plus the v1 fault catalog's…, Missing associated documentation comment in .proto file., Missing associated documentation comment in .proto file., Missing associated documentation comment in .proto file., Steps a lockstep session, or a paused real-time session (single-stepping)., Real-time sessions load paused; Start paces them to the wall clock, Pause stops…, Missing associated documentation comment in .proto file., Missing associated documentation comment in .proto file. (+10 more)

### Community 21 - "Vehicle Assembly"
Cohesion: 0.16
Nodes (26): VtxSection, a_mixed_motor_quad_builds_and_spins_its_motors_differently(), a_quad_without_a_vtx_has_no_video_link(), a_quad_without_video_sections_wires_nothing(), an_open_loop_quad_reports_no_fc_telemetry(), an_open_loop_vehicle_has_its_vtx_at_the_power_up_channel_but_no_osd(), an_unknown_motor_index_is_invalid_argument(), building_b_shadows_the_quad_behind_it() (+18 more)

### Community 22 - "Python Sim Client"
Cohesion: 0.09
Nodes (16): Popen, A connection to one ofs-sim server (and the process, if launched by `launch`).…, Opens the event stream, unless it is open. It also tells the server this client…, Loads a quad. `mode` is "lockstep" (advance with `run`) or "realtime" (loads…, Replaces the *whole* stick state: every argument left out goes back to its…, Steps a lockstep session (or a paused real-time one) by `seconds` of simulated…, Real-time sessions: run paced to the wall clock., The current OSD frame (`present` is False without an OSD). (+8 more)

### Community 23 - "World Config Parsing"
Cohesion: 0.12
Nodes (26): AntennaKind, AntennaSection, check_beamwidth(), check_name(), default_latency_ms(), default_noise_floor_dbm(), default_rhcp(), EmitterSection (+18 more)

### Community 24 - "World Text Writer"
Cohesion: 0.14
Nodes (28): a_nan_pilot_height_is_reported_as_not_finite_only(), a_nan_pilot_height_reports_the_pilot_not_the_derived_handset(), a_world_file_may_place_and_aim_the_handset(), a_world_needs_an_antenna_and_a_known_schema(), an_emitter_needs_one_frequency_in_range(), copy_dir(), every_world_problem_is_reported_at_once(), handset_validation_problems_are_collected() (+20 more)

### Community 25 - "SITL Codec & FDM Packets"
Cohesion: 0.10
Nodes (19): encode_reply(), erpm_block_payload(), FdmPacket, parse_reply_serial(), RcPacket, Result, String, Vec (+11 more)

### Community 26 - "Video Link Stepper"
Cohesion: 0.13
Nodes (20): Emitter, field_drop_probability(), LinkParams, LinkWorld, Outputs, Picture, ramp(), ReceiverAntenna (+12 more)

### Community 27 - "Python Client Facade"
Cohesion: 0.11
Nodes (27): connect(), Emitter, Event, FcTelemetry, _free_port(), Handset, _kind_name(), launch() (+19 more)

### Community 28 - "Grpc"
Cohesion: 0.18
Nodes (30): a_bad_world_path_is_a_config_error_and_loads_nothing(), a_pilot_does_not_outlive_its_session(), a_pilot_that_never_reads_its_states_frees_the_slot_when_its_session_ends(), a_session_flies_in_the_world_it_was_loaded_with(), a_state_stream_without_a_session_fails_at_once(), a_vanished_pilot_switches_the_transmitter_off(), an_abandoned_run_stops_and_releases_the_session(), bad_quad_path_is_a_config_error() (+22 more)

### Community 29 - "M4B V1 Fault Catalog (Seven "
Cohesion: 0.08
Nodes (31): Client delayed composite (FpvBuffer / field_floor / StateBuffer::sim_now), DVR MJPEG AVI muxer (DvrWriter) and Godot capture (dvr.gd), fpv_aspect setting (4:3 / 16:9 / window), Protocol 7 (VideoLink field_valid + latency_ms), FPV composite in a fixed-resolution SubViewport, Per-motor config vectors (motors/props/escs, rotor_inertia Vec), vehicle::Fault enum with set_fault / clear_faults, Fault seams (fault.* bus signals, identity defaults) (+23 more)

### Community 30 - "Vtxmodel"
Cohesion: 0.14
Nodes (14): VtxKind, band_index(), Codec, limits_payload(), Option, Request, Result, Self (+6 more)

### Community 31 - "Model"
Cohesion: 0.10
Nodes (18): Model, Send, argv(), Both, crsf_bytes_in_the_state_datagram_reach_betaflight(), CrsfSource, displayport_frames_come_back_in_the_reply(), esc_telemetry_frames_become_battery_state() (+10 more)

### Community 32 - "Server"
Cohesion: 0.19
Nodes (25): a_call_whose_client_left_before_it_got_the_session_does_nothing(), a_long_run_lets_other_calls_in_between_its_chunks(), a_numerical_failure_is_published_as_a_sim_error_event(), a_panicking_handler_poisons_the_loaded_session(), a_run_stops_when_its_session_is_replaced(), a_session_without_an_osd_answers_get_osd_with_an_absent_frame(), a_watch_cancelled_while_it_waits_for_the_session_leaves_no_watcher(), CancelOnDrop (+17 more)

### Community 33 - "Errors"
Cohesion: 0.09
Nodes (27): Exception, Yields the state at `rate_hz` (1..240) until you stop iterating or the session…, Yields the OSD when it changes (and once at the start), checked at up to…, ConfigError, FirmwareCrashed, from_rpc_error(), InternalError, InvalidArgument (+19 more)

### Community 34 - "Stepctx"
Cohesion: 0.11
Nodes (15): StepCtx, Counter, models_run_in_registration_order_within_a_tick(), NanAt, non_finite_value_stops_the_run_without_advancing(), Reader, Result, Self (+7 more)

### Community 35 - "Open Fpv Sim - Architecture "
Cohesion: 0.10
Nodes (29): FRD-to-FLU sign conventions and AETR channel order (8/8 checks), M0 - Betaflight SITL Spike Implementation Plan, scripts/build-sitl.sh (pinned Betaflight SITL build), M1 plan assumptions A1-A6, M1 - Headless Core Implementation Plan, SITL packet codecs and frame conversion (FdmPacket, RcPacket, ServoPacket, GYRO_SIGN/ACCEL_SIGN), sitl_live integration test (ignored without OFS_SITL_LAUNCH), M3e Video Extras Implementation Plan (+21 more)

### Community 36 - "Crsf"
Cohesion: 0.13
Nodes (16): crc8_dvb_s2(), Decoder, frame(), link_statistics_frame(), LinkStatistics, pack_channels(), parse(), rc_channels_frame() (+8 more)

### Community 37 - "Faults"
Cohesion: 0.07
Nodes (16): BurstLoss, GyroFault, MotorFailure, PropDamage, Faults to inject with `Sim.inject` (spec §6.2): the v1 catalog, radio to video., Every uplink packet is lost for `on_ms`, then the link delivers normally for…, Motor `motor` (0-based) produces `scale` of its torque and current; 0 = dead…, Propeller `prop` (0-based) produces `thrust_scale` of its thrust and torque,… (+8 more)

### Community 38 - "Sitlbridge"
Cohesion: 0.14
Nodes (17): drain_late_replies(), exchange_with_resend(), no_reply(), probe_state_port(), recv_from_ip(), Duration, Error, Ipv4Addr (+9 more)

### Community 39 - "Part"
Cohesion: 0.19
Nodes (25): FrameSection, GroundSection, BatteryHardware, BatteryPart, check_part_schema(), check_provenance(), EscPart, FramePart (+17 more)

### Community 40 - "Vtx"
Cohesion: 0.22
Nodes (20): request_frame(), a_corrupted_request_gets_no_answer(), an_out_of_range_frequency_is_ignored_but_still_answered(), get_settings_is_answered_after_the_latency_with_the_golden_frame(), locking_clears_the_unlocked_bit(), params(), Rig, Vec (+12 more)

### Community 41 - "Worker"
Cohesion: 0.19
Nodes (22): Command, connect_once(), connect_or_launch(), cycle(), Fly, open_pilot(), PilotLink, Arc (+14 more)

### Community 42 - ".Drop()"
Cohesion: 0.10
Nodes (15): Raw, Channel, Drop, Event, JoinHandle, Option, Result, Runtime (+7 more)

### Community 43 - "Pacer"
Cohesion: 0.13
Nodes (16): OverrunPolicy, Pacer, Plan, Self, a_backlog_of_exactly_one_batch_runs(), a_clock_that_goes_backwards_waits(), a_non_finite_clock_runs_nothing(), ahead_of_the_wall_clock_it_sleeps() (+8 more)

### Community 44 - "Videolink Model (Ofs-Video::"
Cohesion: 0.09
Nodes (25): Field drop model (field_drop_probability / FIELD_DROP_P_MAX), VtxInterferenceHandle (runtime emitter injection), Betaflight OSD over MSP DisplayPort (ofs-video::osd), OsdFrame stream (30x16 grid, snapshot on draw), Protocol 3 (StreamOsd, Vtx state message), Analog degradation shader (godot/ui/video.gd), Receiver antenna diversity (2 dB switching hysteresis), Two-ray ground bounce (circular polarization suppression) (+17 more)

### Community 45 - "M3A: Osd And Vtx Control Imp"
Cohesion: 0.10
Nodes (24): CI sitl job (cached SITL binary, sitl_live/vtx_live, five live pytest files, Godot e2e), ofs-fc crate (Betaflight SITL bridge, ESC telemetry codec), ofs-video crate (OSD grid, VTX models, video link), KISS ESC telemetry on UART3 ([esc_telemetry], battery as KISS frames), Betaflight OSD over MSP DisplayPort on UART4 ([osd], 30x16 PAL grid), SmartAudio VTX on UART5 ([vtx] referencing the part), Video timing, DVR and Tramp VTX (M3e features), SITL feature build (CRSF RX, ESC sensor, DisplayPort OSD, SmartAudio, Blackbox) (+16 more)

### Community 46 - "Collision"
Cohesion: 0.28
Nodes (23): a_bounce_raises_one_event_per_touching_spell(), a_dropped_sphere_bounces_to_restitution_squared_of_its_height(), a_legs_only_quad_dropped_on_a_roof_names_the_roof(), a_legs_only_quad_striking_the_ground_raises_the_ground_event(), a_new_touch_within_the_rearm_window_raises_nothing(), a_prop_sphere_clipping_a_post_starts_a_rotation(), a_quad_at_30_mps_into_a_post_never_ends_up_past_it(), a_quad_rests_on_a_roof_with_spheres_and_legs_for_ten_seconds() (+15 more)

### Community 47 - "M2B — The Godot Pilot Client"
Cohesion: 0.14
Nodes (23): CI workflow (core, msrv, windows, godot, sitl jobs), CI core job (clippy -D warnings, cargo test, build ofs-sim, pytest), CI godot job (run-godot-tests.sh all), CI msrv job (cargo check at Rust 1.85, --exclude ofs-godot), CI windows job (cargo test + build + pytest on windows-latest), ofs-client crate (Rust client supervisor, frames, interpolation), ofs-godot crate (Godot GDExtension, rust-version 1.94), ofs-proto crate (generated protocol messages, PROTOCOL_VERSION) (+15 more)

### Community 48 - "Msp"
Cohesion: 0.13
Nodes (18): a_closed_connection_fails_the_request_at_once(), a_corrupt_length_does_not_hide_the_next_frame(), a_payload_longer_than_msp_v1_allows_is_an_error_not_a_panic(), a_rejected_command_is_an_error(), a_stale_reply_is_not_taken_for_the_answer(), fake_fc(), parser_skips_noise_and_bad_checksums(), pump() (+10 more)

### Community 49 - "Test Sitl Video"
Cohesion: 0.14
Nodes (13): far_pilot_world(), first_number(), Msp, OSD, VTX, ESC telemetry and the analog video link through the API against…, The flat field with the pilot 300 m south of the launch pad, looking north at…, The Configurator's VTX tab: band and channel as one index, power level 1..4…, A minimal MSP v1 client on Betaflight's UART1 port; it advances the simulation…, set_vtx() (+5 more)

### Community 50 - "M3D: Flight-Controller Telem"
Cohesion: 0.11
Nodes (21): Betaflight telemetry/crsf.c (CRSF telemetry source of truth), Betaflight Configurator on tcp://127.0.0.1:5761 (only while sim time advances), CRSF telemetry payload encodings (0.1 V battery, radians x 10000 attitude, mode string), 184-byte combined fdm+rc state datagram (UDP 9003), CRSF telemetry atomic-block shim (M3d), Deterministic boot fix ([SITL] ready for the simulator), DShot telemetry stub without USE_DSHOT (RPM filter gate), SITL lockstep patch (ENABLE_SIMULATOR_EXTERNAL_TIME) (+13 more)

### Community 51 - "Propeller"
Cohesion: 0.16
Nodes (15): Propeller, PropParams, Result, Self, String, Vec, thrust_torque(), an_unsorted_coefficient_table_is_refused_when_the_model_is_built() (+7 more)

### Community 52 - "Grpc Sim Service (Proto/Ofs/"
Cohesion: 0.11
Nodes (21): OFS_SITL_CLEANUP (pkill -x betaflight_SITL before/after launch), OFS_SITL_LAUNCH environment variable (SITL argv override), WSL2 NAT networking discovery (WSL VM IP + --ip host IP, OFS_SITL_HOST/OFS_SITL_REPLY_IP), python/examples/hover.py (fly_hover), ofs-error-kind gRPC metadata (typed client errors), Python client package (python/ofs: launch, connect, Sim, typed errors), python/tests suite, Quad TOML config, schema versioning and all-at-once validation (ofs-config) (+13 more)

### Community 53 - "Handles"
Cohesion: 0.16
Nodes (14): env_argv(), Handles, launch_argv_from_the_environment_keeps_quoted_paths_whole(), pairs(), quantize_latency_ms(), Default, Self, String (+6 more)

### Community 54 - "Vehicle"
Cohesion: 0.14
Nodes (9): Fault, injected_and_cleared_faults_are_named_in_order(), Box, Option, OsdFrame, Vehicle, VideoDevices, vtx_interference_without_a_vtx_is_invalid_argument() (+1 more)

### Community 55 - "Ofs Spike"
Cohesion: 0.14
Nodes (16): attitude_ned(), fdm_legacy(), msp_armed(), msp_frames(), qaxis(), qconj(), qmul(), qrot() (+8 more)

### Community 56 - "The Analog Video Link Model "
Cohesion: 0.16
Nodes (19): ofs-config crate (quad and world file parsers), crates/ofs-video/src/link.rs (receiver model on the bus), crates/ofs-video/src/propagation.rs (pure physics functions), RF physics shared with the video link (moved unchanged into ofs-rf), Why FPV uses circular antennas, Golden-section knife-edge obstruction search, The analog video link model (docs/research/video-link.md), SNR-to-picture thresholds (grain, sparkles, colour, tearing, sync lost/relock) (+11 more)

### Community 57 - "Elrslink"
Cohesion: 0.20
Nodes (13): ElrsLink, Inputs, LinkParams, Outputs, packet_error_rate(), rf_mode_index(), ChaCha8Rng, Option (+5 more)

### Community 58 - "M2A - Radio Link, Real-Time "
Cohesion: 0.13
Nodes (19): Failsafe timing (frames stop, failsafe_delay 1.5 s, JUST_DISARM/DROP), M2a - Radio Link, Real-Time Mode and Firmware Lifecycle Implementation Plan, CRSF codec (ofs_radio::crsf, CRC-8/DVB-S2, 11-bit channel packing), ElrsLink ExpressLRS link model (Gilbert-Elliott burst loss, latency, LQ window 100), Protocol-2 gRPC streams (state, bidirectional Pilot link, Watch/event), M2b Godot client plan (next), Milestone M2a (radio link, real time, firmware lifecycle), Real-time Pacer (warn and slow overrun policies) (+11 more)

### Community 59 - "Shared"
Cohesion: 0.19
Nodes (14): lock(), AtomicU64, Instant, Mutex, MutexGuard, Option, OsdFrame, Sender (+6 more)

### Community 60 - "Session"
Cohesion: 0.19
Nodes (13): a_client_connects_loads_and_flies(), a_quad_without_firmware_has_an_absent_osd_but_its_vtx_transmits(), a_second_pilot_is_refused_while_the_client_flies(), a_server_that_goes_away_fails_the_client_as_unavailable(), a_session_unloaded_underneath_the_client_fails_it_and_a_reload_recovers(), climb(), cutting_the_radio_raises_link_events_and_restoring_it_clears_them(), dropping_the_client_turns_the_transmitter_off() (+5 more)

### Community 61 - "Dvec3"
Cohesion: 0.22
Nodes (11): DVec3, Shape, Antenna, AntennaKind, Endpoint, patch_exponent(), Polarization, polarization_loss_db() (+3 more)

### Community 62 - "Simerror"
Cohesion: 0.20
Nodes (17): String, SimError, aim_ned(), antenna_kind(), build(), emitter_freq_mhz(), env_ip(), link_params() (+9 more)

### Community 63 - "Sitl Errors"
Cohesion: 0.20
Nodes (14): a_bind_failure_just_after_the_ready_line_is_a_startup_error(), a_changed_quad_diff_refuses_a_stale_eeprom(), a_diff_that_only_changed_its_line_endings_is_unchanged(), a_relaunch_appends_to_the_sitl_log(), an_eeprom_whose_diff_is_unknown_is_refused(), an_unchanged_diff_reuses_the_eeprom(), booted_with(), busy_pwm_port_is_reported() (+6 more)

### Community 64 - "The Component Library'S Prov"
Cohesion: 0.12
Nodes (18): The component library's provenance (ledger, docs/research/component-library.md), motor.pole_pairs = 7 ruling, prop.imbalance_kgm = 0.0 ruling, Provenance requirement for part parameters, M3d carried debt, Pole-pairs mismatch silently scales the RPM filter input (sim constant 7 vs motor_poles), Ruling: the motor protocol stays PWM, RPM filter spectral proof skipped (too costly for the value) (+10 more)

### Community 65 - "Sitlbridge Lockstep Model (F"
Cohesion: 0.27
Nodes (18): Barometer model (ISA pressure at home altitude + noise), Battery model (Thevenin equivalent circuit), EscMotor averaged ESC + brushless motor (duty lag, current limit, back-EMF), IMU model (gyro/accel noise + bias, per-model seeded RNG), Model trait (typed step, rate divisor, fidelity seam), OpenLoopFc stand-in FC (motor command = clamped throttle), Propeller model (Ct/Cp tables: T = Ct*rho*n^2*D^4), RigidBody 6-DOF model (drag, ground contact, RK2 rotation) (+10 more)

### Community 67 - "Wire"
Cohesion: 0.17
Nodes (8): Fifo, Arc, Mutex, MutexGuard, Self, Vec, VecDeque, Wire

### Community 68 - "Open Fpv Sim (The Project)"
Cohesion: 0.14
Nodes (17): Developer setup (docs/dev-setup.md), Pinned Betaflight 2026.6.2 (commit e0b7bb01), proto/ofs/v1/sim.proto (gRPC protocol definition), quads/opendrone-5f-freestyle.toml (shipped freestyle quad, SmartAudio), quads/opendrone-5f-weak-link.toml, Betaflight SITL in the loop (real firmware), Component library (schema 5 part files with provenance), Geometric ExpressLRS link (RSSI/SNR/LQ from world geometry) (+9 more)

### Community 69 - "Test Sitl Link"
Cohesion: 0.14
Nodes (10): fixture, sim(), sim_bin(), _hover_module(), M1 exit criteria. Needs Betaflight SITL: set OFS_SITL_LAUNCH (cleanup defaults…, Spec §5.5/§9: same quad + seed + inputs in lockstep give identical runs,…, test_betaflight_sitl_hover(), test_betaflight_sitl_hover_is_deterministic() (+2 more)

### Community 70 - "Serverprocess"
Cohesion: 0.16
Nodes (10): read_tail(), Child, Drop, ExitStatus, Option, Path, PathBuf, Result (+2 more)

### Community 71 - "Radioantennasection"
Cohesion: 0.17
Nodes (14): default_radio_antenna_gain_dbi(), default_radio_antenna_mount_frd(), default_radio_antenna_polarization(), default_radio_antennas(), default_vtx_antenna_gain_dbi(), default_vtx_antenna_kind(), default_vtx_mount_frd(), default_vtx_polarization() (+6 more)

### Community 72 - "Names"
Cohesion: 0.23
Nodes (15): esc_bus_current(), esc_duty(), fault_motor_scale(), fault_prop_thrust_scale(), motor_cmd(), motor_current(), motor_omega(), motor_omega_dot() (+7 more)

### Community 73 - "Osd"
Cohesion: 0.38
Nodes (13): a_drawn_screen_is_published_as_text(), a_half_drawn_screen_is_never_shown(), attributes_select_blink_and_the_font_page(), bytes_split_across_pushes_still_decode(), dp(), garbage_is_skipped_and_the_next_good_frame_still_draws(), msp(), Vec (+5 more)

### Community 74 - "Shared"
Cohesion: 0.14
Nodes (10): AtomicUsize, AtomicBool, Instant, IntoIterator, Item, Mutex, MutexGuard, Sender (+2 more)

### Community 75 - "Model"
Cohesion: 0.15
Nodes (5): every_server_error_kind_maps_to_a_typed_error(), Code, Option, status(), transport_failures_and_unknown_kinds_have_their_own_mapping()

### Community 76 - "T A5 Crsf Telemetry"
Cohesion: 0.17
Nodes (7): crc8(), crsf_rc_frame(), kiss_frame(), F, THROWAWAY M3d spike A, final: correct frame names (2026.6 crsf_protocol.h),…, walk(), F

### Community 77 - ".Validate()"
Cohesion: 0.30
Nodes (5): Checker, Problem, Into, IntoIterator, Item

### Community 78 - "Bridge"
Cohesion: 0.25
Nodes (11): BridgeConfig, route_reply(), Vec, SerialLink, SerialTap, take_serial_blocks(), wire_drops(), ReplySerial (+3 more)

### Community 79 - "Sitl Resend"
Cohesion: 0.29
Nodes (10): answered_first_datagram_is_not_resent(), late_duplicate_replies_are_drained_after_a_resend(), Link, lost_first_datagram_is_resent_until_answered(), replies_from_another_address_are_ignored(), JoinHandle, Self, SocketAddr (+2 more)

### Community 80 - "Client"
Cohesion: 0.19
Nodes (6): clamp_rates(), rates_are_clamped_to_what_the_server_and_worker_handle(), Seen, ErrorKind, Option, Phase

### Community 81 - "Part"
Cohesion: 0.27
Nodes (11): a_bad_source_is_a_parse_error(), a_frame_part_loads_with_its_data(), a_motor_part_carries_pole_pairs_and_the_sections_default_the_new_fields(), an_unknown_data_field_is_a_parse_error(), an_unknown_part_schema_version_is_refused(), missing_provenance_names_the_field(), Path, PathBuf (+3 more)

### Community 82 - "Frames"
Cohesion: 0.19
Nodes (11): Option, Self, ServoPacket, fdm_packet(), motor_commands(), rc_channels(), SensorFrame, stick_us() (+3 more)

### Community 83 - "Still Quad()"
Cohesion: 0.19
Nodes (11): Home, argv(), Box, FnOnce, Option, Path, String, Vec (+3 more)

### Community 84 - "Ofs-Sim"
Cohesion: 0.29
Nodes (13): ofs-client, ofs-config, ofs-core, ofs-electrical, ofs-fc, ofs-godot, ofs-physics, ofs-proto (+5 more)

### Community 85 - "M3A Carried Debt"
Cohesion: 0.17
Nodes (12): Betaflight src/main/io/vtx_tramp.c (trampPowerTable datasheet citation), Tramp VTX on UART6 (M3e, frequency-only, built-in power table), SITL UART capture buffers (4096-byte per UART, 512 bytes per reply, serial_overflow), Reply datagram TX capture (dropped count + UART blocks, REPLY_MAX 530, UART1 never captured), M3a carried debt, OSD font atlas GPL-3.0 licensing question (flag to maintainer), serial_overflow never triggered live (testing gap), Tramp deferred (codec behind VtxModel; closed in M3e) (+4 more)

### Community 86 - "Broadcast()"
Cohesion: 0.29
Nodes (10): broadcast(), ConfigError, library_root(), load(), Error, Fn, Path, PathBuf (+2 more)

### Community 88 - "Shape"
Cohesion: 0.24
Nodes (7): a_box_edge_normal_is_the_diagonal_of_its_two_faces(), a_box_face_normal_points_straight_out_of_that_face(), a_cylinder_normals_out_of_its_side_caps_and_rim(), an_inside_box_point_normals_towards_the_closest_face(), Center, close_dir(), Shape

### Community 89 - "Body Shadow Db()"
Cohesion: 0.20
Nodes (6): body_shadow_db(), Diversity, Fader, ChaCha8Rng, close(), the_frame_shadows_the_antenna_when_the_other_end_is_ahead_and_below()

### Community 90 - "Baro"
Cohesion: 0.23
Nodes (7): Baro, BaroParams, isa_pressure_pa(), ChaCha8Rng, Normal, Result, Self

### Community 91 - ".Push()"
Cohesion: 0.33
Nodes (8): crc8(), Request, RequestParser, response_frame(), Option, Vec, settings_payload(), State

### Community 92 - "T6 Windows"
Cohesion: 0.20
Nodes (9): apply_config(), _launch_argv(), launch_sitl(), msp_rc(), First boot: `betaflight_SITL.elf --config betaflight.diff` writes eeprom.bin…, stop_sitl(), THROWAWAY M0 Task 6: Windows-side harness against SITL inside WSL2 (default NAT…, THROWAWAY M3d spike A, take 4: does enabling CRSF telemetry break the CRSF RC… (+1 more)

### Community 93 - "Fnv1A64()"
Cohesion: 0.31
Nodes (6): Fnv1a, fnv1a64(), model_rng(), ChaCha8Rng, Default, Self

### Community 94 - "Battery"
Cohesion: 0.27
Nodes (5): Battery, BatteryParams, Result, Self, Vec

### Community 95 - "Escmotor"
Cohesion: 0.25
Nodes (6): EscMotor, EscParams, MotorParams, Result, Self, String

### Community 96 - "Listen Policy"
Cohesion: 0.24
Nodes (7): check_listen(), Result, SocketAddr, String, addr(), other_addresses_need_the_explicit_flag(), SocketAddr

### Community 98 - "Clienterror"
Cohesion: 0.27
Nodes (6): ClientError, Into, Self, Status, String, End

### Community 99 - "Probe"
Cohesion: 0.31
Nodes (4): Probe, Duration, Fn, Vec

### Community 100 - ".Push()"
Cohesion: 0.33
Nodes (6): crc(), Parser, Request, response_frame(), Option, Vec

### Community 101 - "Test Sitl Telemetry"
Cohesion: 0.24
Nodes (7): RadioLinkLoss, Every radio uplink packet is lost while active: the receiver goes silent and…, _msp_requester(), The FC telemetry chain end to end against real Betaflight (M3d): per-motor eRPM…, Minimal MSP v1 client over the Configurator TCP port, pumped by sim time., test_a_radio_cut_ages_the_telemetry_out(), test_per_motor_erpm_reaches_betaflight()

### Community 102 - "Test Sitl Tramp"
Cohesion: 0.27
Nodes (4): Msp, The Tramp VTX through the API against Betaflight SITL (spec 2026-10-10-m3e §6).…, A minimal MSP v1 client on Betaflight's UART1 port; it advances the simulation…, test_the_tramp_vtx_follows_betaflight()

### Community 103 - "Frames"
Cohesion: 0.28
Nodes (6): quat_to_godot(), vec_to_godot(), a_level_body_facing_north_has_the_godot_camera_looking_north(), close(), pitching_up_raises_the_nose(), yawing_90_degrees_to_the_right_faces_east()

### Community 104 - "Launch"
Cohesion: 0.42
Nodes (8): a_program_that_does_not_exist_is_a_launch_error_with_a_hint(), a_server_that_exits_during_startup_is_reported_with_its_status_and_log(), free_port_addr(), Path, PathBuf, String, spec(), tempfile_dir()

### Community 105 - "Openloopfc"
Cohesion: 0.22
Nodes (4): OpenLoopFc, Result, Self, Vec

### Community 106 - "Kiss Frame()"
Cohesion: 0.31
Nodes (5): crc8(), field(), kiss_frame(), a_frame_has_the_kiss_layout_and_checksum(), values_are_clamped_to_the_field_widths()

### Community 107 - "Fctelemetryhandle"
Cohesion: 0.33
Nodes (5): FcTelemetry, FcTelemetryHandle, Arc, Mutex, String

### Community 108 - "Main"
Cohesion: 0.31
Nodes (8): Args, main(), Box, Error, PathBuf, Result, SocketAddr, shutdown_signal()

### Community 109 - "Runner"
Cohesion: 0.42
Nodes (8): a_panicking_step_poisons_the_session_and_keeps_the_runner_alive(), a_session_that_falls_behind_publishes_an_overrun(), pace_once(), Arc, Duration, JoinHandle, Shared, spawn()

### Community 110 - "Smartaudio"
Cohesion: 0.22
Nodes (3): bad_frames_are_ignored_and_counted(), the_parser_handles_split_input_and_noise_between_frames(), the_parser_reads_every_request_kind()

### Community 111 - "M4A Component Library (Libra"
Cohesion: 0.25
Nodes (9): ofs-config::part module (six part kinds + loaders), Mandatory provenance walker (check_provenance), Quad schema 5 hard cutover (QuadFile / PartRefs), M4a component library (library/ part files, schema 5), Parts by reference only, hard cutover at schema 5, Provenance mandatory per parameter, CC-BY-SA-4.0 part data license, Library part files (one TOML per part, kind directories) (+1 more)

### Community 112 - "T4 Lockstep"
Cohesion: 0.22
Nodes (3): SitlLink, OrderedLink, THROWAWAY M0 Task 4: reply ratio, real-time factor, and reproducibility. Usage:…

### Community 113 - "T B2 Erpm Block"
Cohesion: 0.25
Nodes (3): ErpmFlight, THROWAWAY M3d Task 2: the state datagram's eRPM block reaches Betaflight's…, The state datagram gains one reserved block: [250][len lo][len hi][4 x u16 LE…

### Community 114 - "Esctelemetry"
Cohesion: 0.25
Nodes (4): EscTelemetry, Result, Self, Vec

### Community 115 - "Client Launch"
Cohesion: 0.39
Nodes (7): an_already_running_server_is_used_and_left_running(), free_port_addr(), Duration, Fn, String, the_client_launches_the_server_flies_and_stops_it(), wait()

### Community 116 - "Throwaway M0 Spike Harness ("
Cohesion: 0.25
Nodes (8): Throwaway M0 spike harness (spikes/m0/ofs_spike.py helpers), t2_smoke.py (SITL boots, MSP over TCP, FDM replies), t3_conventions.py (sign conventions via motor responses), t4_lockstep.py (reply ratio, real-time factor, reproducibility), t5_features.py (feature runtime probe over MSP), MSP v1 client (ofs_fc::msp, MspClient pumps the sim while waiting), MSP protocol, ofs-fc crate (Betaflight SITL bridge)

### Community 117 - "Milestone M4 (Sandbox)"
Cohesion: 0.32
Nodes (8): Radio and fault signal names (radio.tx_enabled, radio.link_up, radio.lq_pct, radio.rssi_dbm, fault.radio.link_loss, fc.restarts), Component library with provenance (library/, schema_version, per-part TOML), MCAP recording + Rerun live view + DataFrame export, Milestone M4 (sandbox), ofs-log crate (signal recording), OpenDrone open-hardware initiative, Sandbox (data-defined components, scripting, logging/inspection), v1 fault catalog (motor failure, prop damage, weak cell, packet loss, gyro noise, VTX interference)

### Community 118 - "Osd"
Cohesion: 0.32
Nodes (3): Osd, Betaflight's OSD as a character grid. `cells` are row-major, each `char | page…, The rows as text. Printable ASCII is shown as is; Betaflight's symbols (battery…

### Community 119 - "Test Realtime"
Cohesion: 0.32
Nodes (4): Protocol 2 features without firmware: real-time sessions, streams, faults,…, test_closing_a_client_ends_its_session(), test_radio_link_loss_fault_and_events(), wait_for()

### Community 120 - "Flight"
Cohesion: 0.29
Nodes (5): Flight, Steps a still or scripted vehicle against SITL at a fixed exchange rate., Keep sending the most recent state for `seconds` (used to pump MSP)., rc_packet, channel order AETR + AUX1..4. Sticks in [-1, 1], throttle in [0, 1]., rc()

### Community 122 - "Next Kinds()"
Cohesion: 0.29
Nodes (7): kind(), next_kinds(), Event, Receiver, Status, String, Vec

### Community 123 - "Osd Font.Png Glyph Atlas (Tw"
Cohesion: 0.38
Nodes (7): OSD font page 1 (bold.mcm second atlas page), OSD font atlas converted from Betaflight .mcm, OSD font page 1 drawn from stacked atlas pages, osd_font.png glyph atlas (two stacked pages), Betaflight Configurator bold.mcm font (page 1, INFO severity), Betaflight Configurator default.mcm font (page 0), GPL-3.0 (not 'or later') font licensing

### Community 124 - "Tramp Codec (Crates/Ofs-Vide"
Cohesion: 0.40
Nodes (6): quads/opendrone-5f-tramp.toml (Tramp live-test quad), Tramp codec (crates/ofs-video/src/tramp.rs), VtxKind::Tramp config variant, SmartAudio v2.1 codec, VtxModel (ofs-video), Tramp codec behind the existing VtxModel ([vtx] kind)

### Community 125 - "Erpm Datagram Block (Index 2"
Cohesion: 0.40
Nodes (6): Pole pairs as motor data (POLE_PAIRS constant removed), DShot telemetry emulation without USE_DSHOT (SITL stub), eRPM datagram block (index 250, per-motor eRPM/100), POLE_PAIRS constant (ofs-fc::sitl::esc_telemetry), Motor protocol stays PWM (spike superseded DShot switch), Betaflight RPM filter fed per-motor eRPM

### Community 127 - "T3 Conventions"
Cohesion: 0.40
Nodes (5): msp_arming_disabled(), Names of active arming-disable flags, from MSP_STATUS (layout per…, bias(), check(), THROWAWAY M0 Task 3: verify gyro/accel/attitude conventions through motor…

### Community 128 - "T5 Features"
Cohesion: 0.33
Nodes (3): FeatureFlight, THROWAWAY M0 Task 5: prove each re-enabled feature works at runtime on the…, Sends CRSF RC frames (every 2nd tick) and KISS ESC telemetry (every tick)…

### Community 129 - "Check Table()"
Cohesion: 0.40
Nodes (3): check_table(), Result, String

### Community 130 - "Main()"
Cohesion: 0.40
Nodes (4): main(), Box, Error, Result

### Community 131 - "Firmware Dir()"
Cohesion: 0.40
Nodes (5): BuildOptions, firmware_dir(), Path, PathBuf, firmware_dirs_differ_for_same_named_quads_in_different_folders()

### Community 132 - "Osd Font Atlas (Osd Font"
Cohesion: 0.50
Nodes (5): Betaflight/MSP DisplayPort OSD Font Grid Convention, Dual-Weight Glyph Variant Stack (normal/bold grids), OSD (On-Screen Display) HUD Overlay, OSD Font Atlas (osd_font.png), Telemetry Symbol Set (battery, RSSI, GPS, compass, KM/H-MPH-M-FT units)

### Community 133 - "T B Dshot Telemetry"
Cohesion: 0.40
Nodes (3): motor_telemetry(), THROWAWAY M3d spike B: per-motor eRPM through the emulated DShot telemetry…, MSP_MOTOR_TELEMETRY: u8 count, then per motor u32 rpm, u16 invalidPct, u8 temp,…

### Community 134 - "Make Osd Font"
Cohesion: 0.70
Nodes (4): glyph_pixels(), main(), read_mcm(), write_png()

### Community 135 - "Restartbudget"
Cohesion: 0.67
Nodes (3): RestartBudget, Instant, VecDeque

### Community 138 - "[Collision] Config (6 Sphere"
Cohesion: 0.50
Nodes (4): [collision] config (6 spheres per prop tip circle + body sphere), Impulse contact on collision spheres (deterministic, deepest first), ofs-core::shape (signed distance, normal, bounds), Quad schema 4 (radio antennas, [collision]; rssi/snr fields removed)

### Community 139 - "Hover"
Cohesion: 0.50
Nodes (3): fly_hover(), Arms the quad in Betaflight SITL (angle mode) and holds 1 m with a throttle…, Flies the hover script; returns a summary plus `states`, every State the server…

### Community 140 - "Serve Realtime"
Cohesion: 0.50
Nodes (3): State, Runs the quad with Betaflight SITL in real time until Ctrl+C, so Betaflight…, serve()

## Ambiguous Edges - Review These
- `ELRS receiver model (noise floor -108.9 dBm, per-rate sensitivities, logistic PER, 100-packet LQ window)` → `ELRS figures from ExpressLRS source (SX1280 sensitivities; 50 Hz ruled -115 dBm)`  [AMBIGUOUS]
  docs/superpowers/plans/2026-10-10-m3c-world-collision-elrs-verified.md · relation: conceptually_related_to
- `Ruling: 4 cm body sphere accepted (belly resting is behaviour)` → `Ruling (verified plan): the body sphere is 2.5 cm`  [AMBIGUOUS]
  docs/superpowers/plans/2026-10-10-m3c-world-collision-elrs-verified.md · relation: conceptually_related_to

## Knowledge Gaps
- **108 isolated node(s):** `Reader`, `ofs-godot`, `ofs`, `build-sitl.sh script`, `scripts/build-sitl.sh (pinned Betaflight SITL build)` (+103 more)
  These have ≤1 connection - possible missing edges or undocumented components. (Counts symbols only; 853 node(s) total have ≤1 connection when file, concept and rationale nodes are included.)
- **36 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **What is the exact relationship between `ELRS receiver model (noise floor -108.9 dBm, per-rate sensitivities, logistic PER, 100-packet LQ window)` and `ELRS figures from ExpressLRS source (SX1280 sensitivities; 50 Hz ruled -115 dBm)`?**
  _Edge tagged AMBIGUOUS (relation: conceptually_related_to) - confidence is low._
- **What is the exact relationship between `Ruling: 4 cm body sphere accepted (belly resting is behaviour)` and `Ruling (verified plan): the body sphere is 2.5 cm`?**
  _Edge tagged AMBIGUOUS (relation: conceptually_related_to) - confidence is low._
- **Why does `DVec3` connect `Dvec3` to `Rigid Body Physics`, `ELRS Scheduler & Frames`, `FPV Interpolation Buffer`, `Session & Events`, `Client Data Model`, `Signal Bus Core`, `IMU & Vibration Sensors`, `RF Propagation`, `VTX Paths & Interference`, `Vehicle Assembly`, `Video Link Stepper`, `Collision`, `Handles`, `Vehicle`, `Elrslink`, `Simerror`, `Frames`, `Shape`, `Body Shadow Db()`, `Baro`, `Frames`?**
  _High betweenness centrality (0.134) - this node is a cross-community bridge._
- **Why does `Bus` connect `Signal Bus Core` to `MSP OSD Client`, `Rigid Body Physics`, `ELRS Scheduler & Frames`, `.New()`, `IMU & Vibration Sensors`, `VTX Paths & Interference`, `Vehicle Assembly`, `Video Link Stepper`, `Vtxmodel`, `Model`, `Stepctx`, `Sitlbridge`, `Vtx`, `Propeller`, `Handles`, `Elrslink`, `Dvec3`, `Still Quad()`, `Baro`, `Battery`, `Escmotor`, `Openloopfc`, `Esctelemetry`?**
  _High betweenness centrality (0.068) - this node is a cross-community bridge._
- **Why does `SimError` connect `Simerror` to `MSP OSD Client`, `Rigid Body Physics`, `ELRS Scheduler & Frames`, `SimService gRPC Surface`, `Session & Events`, `IMU & Vibration Sensors`, `Vehicle Assembly`, `Video Link Stepper`, `Vtxmodel`, `Model`, `Stepctx`, `Sitlbridge`, `Msp`, `Propeller`, `Vehicle`, `Elrslink`, `Baro`, `Battery`, `Escmotor`, `Openloopfc`, `Esctelemetry`?**
  _High betweenness centrality (0.037) - this node is a cross-community bridge._
- **Are the 39 inferred relationships involving `load()` (e.g. with `a_collision_section_is_honoured()` and `a_diff_that_enables_smartaudio_needs_a_vtx_section()`) actually correct?**
  _`load()` has 39 INFERRED edges - model-reasoned connections that need verification._
- **What connects `Reader`, `ofs-godot`, `ofs` to the rest of the system?**
  _108 weakly-connected nodes found - possible documentation gaps or missing edges._