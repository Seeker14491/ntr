//! ## Example
//!
//! The following program interfaces with Monster Hunter Generations (USA). It sets the large
//! monster's health to 1000, then displays the monster's health every second until its health
//! reaches 0.
//!
//! ```no_run
//! extern crate ntr;
//!
//! use std::thread;
//! use std::time::Duration;
//! use ntr::Connection;
//!
//! // ip of N3DS to connect to
//! const N3DS_IP: &'static str = "192.168.0.12";
//!
//! // title id for Monster Hunter Generations (USA); list can be found at http://3dsdb.com/
//! const MH_TID: u64 = 0x0004000000187000;
//!
//! // addresses we'll use (Credit: ymyn)
//! const MONSTER_1_PTR: u32 = 0x83343A4;
//! const HEALTH_OFFSET: u32 = 0x1318;
//!
//! fn main() {
//!     println!("Connecting to {}", N3DS_IP);
//!     let mut connection = Connection::new(N3DS_IP).unwrap();
//!     print!("Connected.\n\n");
//!
//!     // get process id using title id
//!     let pid = connection.get_pid(MH_TID)
//!         .expect("io error")
//!         .expect("pid not found");
//!
//!     // go through a pointer to get the health address
//!     let health_address = connection.read_u32(MONSTER_1_PTR, pid).unwrap() + HEALTH_OFFSET;
//!
//!     // set monster's health to 1000
//!     connection.write_u32(health_address, 1000, pid).unwrap();
//!
//!     // monster health printing
//!     loop {
//!         let health = connection.read_u32(health_address, pid).unwrap();
//!         if health > 0 {
//!             println!("First monster's health: {}\n", health);
//!             thread::sleep(Duration::from_secs(1));
//!         } else {
//!             println!("First monster is slain!");
//!             break;
//!         }
//!     }
//! }
//! ```

#![warn(missing_copy_implementations, missing_debug_implementations, missing_docs,
    unused_extern_crates, unused_import_braces, unused_qualifications)]

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
#[derive(Debug)]
pub struct Connection {
    ntr_sender: Arc<Mutex<NtrSender>>,
    mem_read_rx: Receiver<Box<[u8]>>,
    get_pid_rx: Receiver<String>,
}

impl Connection {
    /// Opens a connection to the 3DS with the address `addr`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ntr::Connection;
    ///
    /// let mut connection = Connection::new("192.168.2.247").expect("io error");
    /// ```
    pub fn new(addr: &str) -> io::Result<Self> {
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

        Ok(Connection {
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
    /// use ntr::Connection;
    ///
    /// # let mut connection: Connection = unimplemented!();
    /// let pid = connection.get_pid(0x0004000000126300u64)
    ///     .expect("io error")
    ///     .expect("pid not found");
    /// ```
    pub fn get_pid(&mut self, tid: u64) -> io::Result<Option<u32>> {
        try!(self.ntr_sender.lock().unwrap().send_list_process_packet());
        let msg = self.get_pid_rx.recv().unwrap();
        let cap = {
            let mut re = r"pid: 0x([0-9a-fA-F]{8}), pname:[^,]*, tid: ".to_owned();
            re.push_str(&format!("{:016x}", tid));
            Regex::new(&re).unwrap().captures(&msg)
        };
        Ok(cap.and_then(|x| Some(u32::from_str_radix(x.at(1).unwrap(), 16).unwrap())))
    }

    /// Reads a chunk of 3DS memory.
    ///
    /// Reads `size` bytes of 3DS memory starting from address `addr` for the
    /// process with process id `pid`.
    pub fn mem_read(&mut self, addr: u32, size: u32, pid: u32) -> io::Result<Box<[u8]>> {
        try!(self.ntr_sender.lock().unwrap().send_mem_read_packet(addr, size, pid));
        Ok(self.mem_read_rx.recv().unwrap())
    }

    /// Writes data to 3DS memory.
    ///
    /// Writes `data` to the 3DS memory starting at address `addr` for the
    /// process with process id `pid`.
    pub fn mem_write(&mut self, addr: u32, data: &[u8], pid: u32) -> io::Result<usize> {
        self.ntr_sender.lock().unwrap().send_mem_write_packet(addr, pid, data)
    }

    /// Reads a `u32` from 3DS memory.
    pub fn read_u32(&mut self, addr: u32, pid: u32) -> io::Result<u32> {
        Ok(LittleEndian::read_u32(&try!(self.mem_read(addr, 4, pid))))
    }

    /// Reads a `u16` from 3DS memory.
    pub fn read_u16(&mut self, addr: u32, pid: u32) -> io::Result<u16> {
        Ok(LittleEndian::read_u16(&try!(self.mem_read(addr, 2, pid))))
    }

    /// Reads a `u8` from 3DS memory.
    pub fn read_u8(&mut self, addr: u32, pid: u32) -> io::Result<u8> {
        Ok(try!(self.mem_read(addr, 1, pid))[0])
    }

    /// Reads an `i32` from 3DS memory.
    pub fn read_i32(&mut self, addr: u32, pid: u32) -> io::Result<i32> {
        Ok(LittleEndian::read_i32(&try!(self.mem_read(addr, 4, pid))))
    }

    /// Reads an `i16` from 3DS memory.
    pub fn read_i16(&mut self, addr: u32, pid: u32) -> io::Result<i16> {
        Ok(LittleEndian::read_i16(&try!(self.mem_read(addr, 2, pid))))
    }

    /// Reads an `i8` from 3DS memory.
    pub fn read_i8(&mut self, addr: u32, pid: u32) -> io::Result<i8> {
        Ok(try!(self.mem_read(addr, 1, pid))[0] as i8)
    }

    /// Writes a `u32` to 3DS memory.
    pub fn write_u32(&mut self, addr: u32, data: u32, pid: u32) -> io::Result<()> {
        let buf = &mut vec![0u8; 4];
        LittleEndian::write_u32(buf, data);
        self.mem_write(addr, buf, pid).map(|_| ())
    }

    /// Writes a `u16` to 3DS memory.
    pub fn write_u16(&mut self, addr: u32, data: u16, pid: u32) -> io::Result<()> {
        let buf = &mut vec![0u8; 2];
        LittleEndian::write_u16(buf, data);
        self.mem_write(addr, buf, pid).map(|_| ())
    }

    /// Writes a `u8` to 3DS memory.
    pub fn write_u8(&mut self, addr: u32, data: u8, pid: u32) -> io::Result<()> {
        self.mem_write(addr, &[data], pid).map(|_| ())
    }

    /// Writes an `i32` to 3DS memory.
    pub fn write_i32(&mut self, addr: u32, data: i32, pid: u32) -> io::Result<()> {
        let buf = &mut vec![0u8; 4];
        LittleEndian::write_i32(buf, data);
        self.mem_write(addr, buf, pid).map(|_| ())
    }

    /// Writes an `i16` to 3DS memory.
    pub fn write_i16(&mut self, addr: u32, data: i16, pid: u32) -> io::Result<()> {
        let buf = &mut vec![0u8; 2];
        LittleEndian::write_i16(buf, data);
        self.mem_write(addr, buf, pid).map(|_| ())
    }

    /// Writes an `i8` to 3DS memory.
    pub fn write_i8(&mut self, addr: u32, data: i8, pid: u32) -> io::Result<()> {
        self.mem_write(addr, &[data as u8], pid).map(|_| ())
    }
}
