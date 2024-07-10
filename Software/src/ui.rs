use std::sync::mpsc::{Receiver, Sender};

use crate::{config::KNOWN_NUMBERS, hardware};

pub fn ui_entry(web_sender: Sender<(i32, String)>, _web_reciever: Receiver<i32>) {
    let mut hardware = hardware::create();

    hardware.ring(true);
    hardware.enable_dialing(true);

    let mut in_call = false;
    let mut last_hook_state = true;

    loop {
        hardware.update();

        let hook_state = hardware.get_hook_state();

        if !hardware.dialed_number.is_empty() {
            let mut contains = false;

            for number in KNOWN_NUMBERS {
                if number == hardware.dialed_number {
                    contains = true;
                }
            }

            if !contains {
                for number in KNOWN_NUMBERS {
                    if number.starts_with(&hardware.dialed_number) {
                        contains = true;
                    }
                }

                if !contains {
                    hardware.dialed_number = String::from("0");
                }

                contains = false;
            }

            if contains {
                hardware.enable_dialing(false);

                if hook_state {
                    hardware.ring(true);
                }

                in_call = true;

                println!("Calling: {}", hardware.dialed_number);
                let _ = web_sender.send((1, hardware.dialed_number.clone()));

                hardware.dialed_number.clear();
            }
        }

        if last_hook_state != hook_state {
            if hook_state {
                if in_call {
                    in_call = false;
                    
                    hardware.enable_dialing(true);

                    println!("Call Ended.");
                    let _ = web_sender.send((0, String::new()));
                }
            } else if in_call {
                hardware.ring(false);
            }
        }

        last_hook_state = hook_state;
    }
}
