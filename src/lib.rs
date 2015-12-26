//! ## Example
//!
//! The following program interfaces with Monster Hunter 4 Ultimate (USA). It sets the large
//! monster's health to 5000, then displays the monster's health every second until its health
//! reaches 0. This program also uses the [byteorder crate](https://crates.io/crates/byteorder).
//!
//! ```no_run
//! extern crate ntr;
//! extern crate byteorder;
//!
//! use std::thread;
//! use ntr::Ntr;
//! use byteorder::{ByteOrder, LittleEndian};
//!
//! // ip of N3DS to connect to
//! const N3DS_IP: &'static str = "192.168.2.247";
//!
//! // title id for Monster Hunter 4 Ultimate (USA); list can be found at http://3dsdb.com/
//! const MH_TID: u64 = 0x0004000000126300;
//!
//! fn main() {
//!     println!("Connecting to {}", N3DS_IP);
//!     let mut ntr = Ntr::connect(N3DS_IP).unwrap();
//!     print!("Connected.\n\n");
//!
//!     // get process id using title id
//!     let pid = ntr.get_pid(MH_TID)
//!         .expect("io error")
//!         .expect("pid not found");
//!
//!     // go through a few pointers to get the health address
//!     let health_address = {
//!         let p = LittleEndian::read_u32(&ntr.mem_read(0x081C7D00, 4, pid).unwrap());
//!         LittleEndian::read_u32(&ntr.mem_read(p + 0xE28, 4, pid).unwrap()) + 0x3E8
//!     };
//!
//!     // set monster's health to 5000
//!     {
//!         let buf = &mut vec![0u8; 4];
//!         LittleEndian::write_u32(buf, 5000);
//!         ntr.mem_write(health_address, buf, pid).unwrap();
//!     }
//!
//!
//!     // monster health printing
//!     loop {
//!         let health = LittleEndian::read_u32(&ntr.mem_read(health_address, 4, pid).unwrap());
//!         if health > 0 {
//!             println!("First monster's health: {}\n", health);
//!             thread::sleep_ms(1000);
//!         } else {
//!             println!("First monster is dead!");
//!             break;
//!         }
//!
//!     }
//! }
//! ```

#![warn(missing_copy_implementations, missing_docs,
    unused_extern_crates, unused_import_braces, unused_qualifications)]
#![feature(plugin)]
#![plugin(clippy)]

extern crate byteorder;
extern crate regex;
extern crate time;

mod ntr_sender;

use std::io;
use std::io::prelude::*;
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use std::thread;
use std::time::Duration;
use byteorder::{ByteOrder, LittleEndian};
use regex::Regex;
use time::PreciseTime;

use ntr_sender::NtrSender;

/// A connection to a 3DS.
pub struct Ntr {
    ntr_sender: Arc<Mutex<NtrSender>>,
    mem_read_rx: Receiver<Box<[u8]>>,
    get_pid_rx: Receiver<String>,
}
impl Ntr {
    /// Opens a connection to the 3DS with the address `addr`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ntr::Ntr;
    ///
    /// let mut ntr = Ntr::connect("192.168.2.247").expect("io error");
    /// ```
    pub fn connect(addr: &str) -> io::Result<Self> {
        let mut tcp_stream = try!(TcpStream::connect(&(addr.to_owned() + ":8000") as &str));
        let (mem_read_tx, mem_read_rx) = mpsc::channel();
        let (get_pid_tx, get_pid_rx) = mpsc::channel();

        let ntr_sender = Arc::new(Mutex::new(NtrSender::new(try!(tcp_stream.try_clone()))));

        // spawn heartbeat thread
        {
            let ntr_sender = ntr_sender.clone();
            thread::spawn(move || {
                let one_second = time::Duration::seconds(1);
                let mut heartbeat_sent_time = PreciseTime::now();
                loop {
                    let mut ntr_sender = ntr_sender.lock().unwrap();
                    if heartbeat_sent_time.to(PreciseTime::now()) >= one_second &&
                       ntr_sender.is_heartbeat_sendable() {
                        ntr_sender.send_heartbeat_packet().unwrap();
                        heartbeat_sent_time = PreciseTime::now();
                        ntr_sender.set_is_heartbeat_sendable(false);
                    }
                    drop(ntr_sender);
                    thread::sleep(Duration::from_millis(500));
                }
            });
        }

        // spawn receiver thread
        {
            let ntr_sender = ntr_sender.clone();
            thread::spawn(move || {
                let mut buf = [0u8; 84];
                loop {
                    tcp_stream.read_exact(&mut buf).unwrap();
                    let cmd = LittleEndian::read_u32(&buf[12..16]);
                    let data_len = LittleEndian::read_u32(&buf[80..84]) as usize;

                    if cmd == 0 {
                        ntr_sender.lock().unwrap().set_is_heartbeat_sendable(true);
                    }
                    if data_len != 0 {
                        let mut data_buf = vec![0u8; data_len].into_boxed_slice();
                        tcp_stream.read_exact(&mut data_buf).unwrap();

                        if cmd == 0 {
                            let msg = String::from_utf8_lossy(&data_buf);
                            if let Some(_) = msg.find("end of process list.") {
                                get_pid_tx.send(msg.into_owned()).unwrap();
                            }
                        } else if cmd == 9 {
                            mem_read_tx.send(data_buf).unwrap();
                        }
                    }
                }
            });
        }

        Ok(Ntr {
            ntr_sender: ntr_sender,
            mem_read_rx: mem_read_rx,
            get_pid_rx: get_pid_rx,
        })
    }

    /// Returns the process identifier for the currently running title id `tid`.
    ///
    /// You can find a list of title ids for 3DS games at [3dsdb](http://3dsdb.com/).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ntr::Ntr;
    ///
    /// # let mut ntr: Ntr = unimplemented!();
    /// let pid = ntr.get_pid(0x0004000000126300u64)
    ///     .expect("io error")
    ///     .expect("pid not found");
    /// ```
    pub fn get_pid(&mut self, tid: u64) -> io::Result<Option<u32>> {
        try!(self.ntr_sender.lock().unwrap().send_list_process_packet());
        let msg = self.get_pid_rx.recv().unwrap();
        let cap = {
            let mut re = r"pid: 0x(\d{8}), pname:[^,]*, tid: ".to_owned();
            re.push_str(&format!("{:016x}", tid));
            Regex::new(&re).unwrap().captures(&msg)
        };
        Ok(cap.and_then(|x| Some(u32::from_str_radix(x.at(1).unwrap(), 16).unwrap())))
    }

    /// Reads a chunk of 3DS memory.
    ///
    /// This function reads `size` bytes of 3DS memory starting from address `addr` for the
    /// process with process id `pid`.
    pub fn mem_read(&mut self, addr: u32, size: u32, pid: u32) -> io::Result<Box<[u8]>> {
        try!(self.ntr_sender.lock().unwrap().send_mem_read_packet(addr, size, pid));
        Ok(self.mem_read_rx.recv().unwrap())
    }

    /// Writes data to 3DS memory.
    ///
    /// This function writes `data` to the 3DS memory starting at address `addr` for the
    /// process with process id `pid`.
    pub fn mem_write(&mut self, addr: u32, data: &[u8], pid: u32) -> io::Result<usize> {
        self.ntr_sender.lock().unwrap().send_mem_write_packet(addr, pid, data)
    }
}
