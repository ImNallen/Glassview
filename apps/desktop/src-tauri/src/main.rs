#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(dead_code)]

mod error;
mod keys;
mod layout;
mod overlays;
mod pipeline;
mod preference_saves;
mod preferences;
mod session;
mod state;

use error::Result;

fn main() {}
