use std::{
    sync::mpsc::{Receiver, Sender},
    time::Duration,
};

pub fn web_entry(_ui_sender: Sender<i32>, _ui_reciever: Receiver<(i32, String)>) {
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}
