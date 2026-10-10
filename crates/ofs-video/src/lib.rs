//! Video-side models: the SmartAudio VTX, Betaflight's OSD over MSP DisplayPort, and the 5.8 GHz analog link from
//! the VTX to the pilot's goggles (the RF maths itself lives in `ofs-rf`).
pub mod link;
pub mod osd;
pub mod smartaudio;
pub mod vtx;
