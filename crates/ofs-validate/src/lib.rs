//! The validation harness (parent spec §8.5): a defined format for community measurements, a check that a submission
//! follows it, and a comparison of a thrust-stand submission with the simulator's own motor and propeller models.
//! Tracked, not gating: a valid submission always yields a report, whatever the error in it.
pub mod report;
pub mod submission;
pub mod thrust;
