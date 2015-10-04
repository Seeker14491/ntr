# ntr

[![Build Status](https://travis-ci.org/Seeker14491/ntr.svg?branch=master)](https://travis-ci.org/Seeker14491/ntr)

A Rust library for interfacing with the debugging functionality of [NTR CFW](https://gbatemp.net/threads/release-ntr-cfw-2-2-anti-piracy-region-free-cfw-on-jp-eu-us-aus-new-3ds.385142/). It requires Rust nightly to build.

## Example

The following program interfaces with Monster Hunter 4 Ultimate (USA). It sets the large monster's health to 5000, then displays the monster's health every second until its health reaches 0. This program also uses the [byteorder crate](https://crates.io/crates/byteorder).

```rust
extern crate ntr;
extern crate byteorder;

use std::thread;
use ntr::Ntr;
use byteorder::{ByteOrder, LittleEndian};

// ip of N3DS to connect to
const N3DS_IP: &'static str = "192.168.2.247";

// title id for Monster Hunter 4 Ultimate (USA); list can be found at http://3dsdb.com/
const MH_TID: u64 = 0x0004000000126300;

fn main() {
    println!("Connecting to {}", N3DS_IP);
    let mut ntr = Ntr::connect(N3DS_IP).unwrap();
    print!("Connected.\n\n");

    // get process id using title id
    let pid = ntr.get_pid(MH_TID)
        .expect("io error")
        .expect("pid not found");

    // go through a few pointers to get the health address
    let health_address = {
        let p = LittleEndian::read_u32(&ntr.mem_read(0x081C7D00, 4, pid).unwrap());
        LittleEndian::read_u32(&ntr.mem_read(p + 0xE28, 4, pid).unwrap()) + 0x3E8
    };

    // set monster's health to 5000
    {
        let buf = &mut vec![0u8; 4];
        LittleEndian::write_u32(buf, 5000);
        ntr.mem_write(health_address, buf, pid).unwrap();
    }


    // monster health printing
    loop {
        let health = LittleEndian::read_u32(&ntr.mem_read(health_address, 4, pid).unwrap());
        if health > 0 {
            println!("First monster's health: {}\n", health);
            thread::sleep_ms(1000);
        } else {
            println!("First monster is dead!");
            break;
        }

    }
}
```
