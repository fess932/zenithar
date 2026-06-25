// Desktop entry. All logic lives in lib.rs so mobile (Android/iOS) can share it
// via the `mobile_entry_point`. Keep this thin.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    zenithar_app_lib::run();
}
