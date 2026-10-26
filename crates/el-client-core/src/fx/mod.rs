//! Effects and sound, as data and decisions (`docs/FX.md`): what every ability looks and sounds
//! like ([`look`], [`sound`]), what a frame should show and play from what the client was told
//! ([`director`]), how the sounds share the speakers ([`mix`]), and the announcer and the music
//! ([`voice`]). The browser client draws and plays it; all of it is tested here.

pub mod director;
pub mod look;
pub mod mix;
pub mod sound;
pub mod voice;
