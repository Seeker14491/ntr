#![feature(read_exact)]

mod ntr_sender;

extern crate byteorder;
extern crate regex;
extern crate time;

use std::io;
use std::io::prelude::*;
use std::mem;
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use std::thread;
use byteorder::{ByteOrder, LittleEndian};
use regex::Regex;
use time::{Duration, PreciseTime};

use ntr_sender::NtrSender;

pub struct Ntr {
    ntr_sender: Arc<Mutex<NtrSender>>,
    mem_read_rx: Receiver<Vec<u8>>,
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
                let one_second = Duration::seconds(1);
                let mut heartbeat_sent_time = PreciseTime::now();
                loop {
                    let mut ntr_sender = ntr_sender.lock().unwrap();
                    if heartbeat_sent_time.to(PreciseTime::now()) >= one_second &&
                     ntr_sender.is_heartbeat_sendable() {
                        ntr_sender.send_heartbeat_packet().unwrap();
                        heartbeat_sent_time = PreciseTime::now();
                        ntr_sender.set_is_heartbeat_sendable(false);
                    }
                    mem::drop(ntr_sender);
                    thread::sleep_ms(500);
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
                        let mut data_buf = vec![0u8; data_len];
                        tcp_stream.read_exact(&mut data_buf[0..data_len]).unwrap();

                        if cmd == 0 {
                            let msg = String::from_utf8(data_buf).unwrap();
                            if let Some(_) = msg.find("end of process list.") {
                                get_pid_tx.send(msg).unwrap();
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

    /// Return the process identifier for the currently running title id `tid`.
    ///
    /// You can find a list of title ids for 3DS games at [3dsdb](http://3dsdb.com/).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ntr::Ntr;
    ///
    /// # let mut ntr: Ntr;
    /// # unsafe { ntr = std::mem::uninitialized::<Ntr>(); }
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
            Regex::new(&re)
                .unwrap()
                .captures(&msg)
        };
        Ok(match cap {
            Some(x) => Some(u32::from_str_radix(x.at(1).unwrap(), 16).unwrap()),
            None => None,
        })
    }

    /// Read a chunk of 3DS memory.
    ///
    /// This function reads `size` bytes of 3DS memory starting from address `addr` for the
    /// process with process id `pid`.
    pub fn mem_read(&mut self, addr: u32, size: u32, pid: u32) -> io::Result<Vec<u8>> {
        try!(self.ntr_sender.lock().unwrap().send_mem_read_packet(addr, size, pid));
        Ok(self.mem_read_rx.recv().unwrap())
    }

    /// Write data to 3DS memory.
    ///
    /// This function writes `data` to the 3DS memory starting at address `addr` for the
    /// process with process id `pid`.
    pub fn mem_write(&mut self, addr: u32, data: &Vec<u8>, pid: u32) -> io::Result<usize> {
        self.ntr_sender.lock().unwrap().send_mem_write_packet(addr, pid, data)
    }
}