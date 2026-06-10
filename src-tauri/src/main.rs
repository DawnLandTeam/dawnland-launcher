// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![allow(unused)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[tokio::main]
async fn main() {
    dawnland_launcher_lib::run()
}
