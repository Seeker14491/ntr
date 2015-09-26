#![allow(dead_code)]
#![feature(read_exact)]

mod ntr_sender;
use ntr_sender::NtrSender;

extern crate byteorder;
use byteorder::{ByteOrder, LittleEndian};

extern crate time;
use time::{Duration, PreciseTime};

use std::mem;
use std::io;
use std::io::prelude::*;
use std::net::TcpStream;
use std::thread;
use std::sync::{Arc, Mutex};
use std::sync::mpsc;
use std::sync::mpsc::Receiver;

pub struct Ntr {
    ntr_sender: Arc<Mutex<NtrSender>>,
    mem_read_rx: Receiver<Vec<u8>>,
}
impl Ntr {
    pub fn connect(addr: &str) -> io::Result<Self> {
        let mut tcp_stream = try!(TcpStream::connect(&(addr.to_owned() + ":8000") as &str));
        let (mem_read_tx, mem_read_rx) = mpsc::channel();

        let ntr = {
            let ntr_sender = NtrSender::new(tcp_stream.try_clone().unwrap());
            Ntr {
                ntr_sender: Arc::new(Mutex::new(ntr_sender)),
                mem_read_rx: mem_read_rx,
            }
        };

        // spawn heartbeat thread
        {
            let ntr_sender = ntr.ntr_sender.clone();
            thread::spawn(move || {
                let one_second = Duration::seconds(1);
                let mut heartbeat_sent_time = PreciseTime::now();
                loop {
                    let mut ntr_sender = ntr_sender.lock().unwrap();
                    if heartbeat_sent_time.to(PreciseTime::now()) >= one_second
                     && ntr_sender.is_heartbeat_sendable() {
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
            let ntr_sender = ntr.ntr_sender.clone();
            thread::spawn(move || {
                let mut buf = [0u8; 84];
                loop {
                    tcp_stream.read_exact(&mut buf).unwrap();
                    let cmd = LittleEndian::read_u32(&buf[12..16]);
                    let data_len = LittleEndian::read_u32(&buf[80..84]) as usize;

                    if data_len != 0 {
                        let mut data_buf = vec![0u8; data_len];
                        tcp_stream.read_exact(&mut data_buf[0..data_len]).unwrap();
                        if cmd == 0 {
                            ntr_sender.lock().unwrap().set_is_heartbeat_sendable(true);
                        } else if cmd == 9 {
                            mem_read_tx.send(data_buf).unwrap();
                        }
                    }
                }
            });
        }

        Ok(ntr)
    }

    pub fn mem_read(&mut self, addr: u32, size: u32, pid: u32) -> Result<Vec<u8>, ()> {
        try!(self.ntr_sender.lock().unwrap().send_mem_read_packet(addr, size, pid).map_err(|_x| ()));
        self.mem_read_rx.recv().map_err(|_x| ())
    }

    pub fn mem_write(&mut self, addr: u32, data: &Vec<u8>, pid: u32) -> io::Result<usize> {
        self.ntr_sender.lock().unwrap().send_mem_write_packet(addr, pid, data)
    }
}