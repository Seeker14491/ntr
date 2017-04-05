// The following program interfaces with Monster Hunter Generations (USA). It sets the large
// monster's health to 1000, then displays the monster's health every second until its health
// reaches 0.

extern crate ntr;

use ntr::Connection;
use std::thread;
use std::time::Duration;

// ip of N3DS to connect to
const N3DS_IP: &'static str = "192.168.2.210";

// title id for Monster Hunter Generations (USA); list can be found at http://3dsdb.com/
const MH_TID: u64 = 0x0004000000187000;

// addresses we'll use (Credit: ymyn)
const MONSTER_1_PTR: u32 = 0x83343A4;
const HEALTH_OFFSET: u32 = 0x1318;

fn main() {
    println!("Connecting to {}", N3DS_IP);
    let mut connection = Connection::new(N3DS_IP).unwrap();
    print!("Connected.\n\n");

    // get process id using title id
    let pid = connection
        .get_pid(MH_TID)
        .expect("io error")
        .expect("pid not found");

    // go through a pointer to get the health address
    let health_address = connection.read_u32(MONSTER_1_PTR, pid).unwrap() + HEALTH_OFFSET;
    let initial_health = connection.read_u32(health_address, pid).unwrap();
    println!("Health address: {:x}\nInitial health: {}", health_address, initial_health);

    // set monster's health to 1000
    connection.write_u32(health_address, 1000, pid).unwrap();

    // monster health printing
    loop {
        let health = connection.read_u32(health_address, pid).unwrap();
        if health > 0 {
            println!("First monster's health: {}\n", health);
            thread::sleep(Duration::from_secs(1));
        } else {
            println!("First monster is slain!");
            break;
        }
    }
}
