pub mod config;
pub mod hardware;
pub mod ui;
pub mod web;

use std::{sync::mpsc::channel, thread};

use web::web_entry;

use crate::ui::ui_entry;

fn main() {
    let (web_tx, ui_rx) = channel::<i32>();
    let (ui_tx, web_rx) = channel::<(i32, String)>();

    thread::spawn(|| {
        web_entry(web_tx, web_rx);
    });

    ui_entry(ui_tx, ui_rx);
}
