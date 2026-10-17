//! Entry point of the Carafe Lab window.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    lab_desktop_lib::run();
}
