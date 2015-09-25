#![allow(dead_code)]
#![allow(unused_variables)]

#![feature(read_exact)]

extern crate byteorder;
extern crate time;

use std::io;
use std::io::prelude::*;
use std::net::TcpStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use byteorder::{ByteOrder, LittleEndian};
use time::{Duration, PreciseTime};

pub struct Ntr {
    tcp_stream: TcpStream,
    current_seq: AtomicUsize,
    is_heartbeat_sendable: AtomicBool,
    mem_read_rx: Receiver<Vec<u8>>,
}
unsafe impl Sync for Ntr {} // TODO: fix this properly
impl Ntr {
    pub fn connect(address: &str) -> io::Result<Arc<Self>> {
        let tcp_stream = try!(TcpStream::connect(&(address.to_owned() + ":8000") as &str));
        let (mem_read_tx, mem_read_rx) = mpsc::channel();

        let ntr = Arc::new(Ntr {
            tcp_stream: tcp_stream,
            current_seq: AtomicUsize::new(1000),
            is_heartbeat_sendable: AtomicBool::new(true),
            mem_read_rx: mem_read_rx,
        });

        // spawn receiver thread
        {
            let ntr = ntr.clone();
            thread::spawn(move || {
                let mut ntr_stream = NtrStream::new(&ntr);
                let mut buf = [0u8; 84];
                loop {
                    if let Err(_) = ntr_stream.tcp_stream().read_exact(&mut buf) {
                        break;
                    }
                    let cmd = LittleEndian::read_u32(&buf[12..16]);
                    let data_len = LittleEndian::read_u32(&buf[80..84]) as usize;

                    if data_len != 0 {
                        let mut data_buf = vec![0u8; data_len];
                        ntr_stream.tcp_stream().read_exact(&mut data_buf[0..data_len]).unwrap();
                        if cmd == 0 {
                            ntr_stream.is_heartbeat_sendable().store(true, Ordering::SeqCst);
                        } else if cmd == 9 {
                            mem_read_tx.send(data_buf).unwrap();
                        }
                    }
                }
            });
        }

        // spawn heartbeat thread
        /*{
            let ntr = ntr.clone();
            thread::spawn(move || {
                let mut ntr_stream = NtrStream::new(&ntr);
                let one_second = Duration::seconds(1);
                let mut heartbeat_sent_time = PreciseTime::now();
                ntr_stream.send_heartbeat_packet().unwrap();
                loop {
                    if heartbeat_sent_time.to(PreciseTime::now()) >= one_second
                     && ntr_stream.is_heartbeat_sendable().compare_and_swap(true, false, Ordering::SeqCst) {
                        ntr_stream.send_heartbeat_packet().unwrap();
                        heartbeat_sent_time = PreciseTime::now();
                    }
                    thread::sleep_ms(100);
                }
            });
        }*/

        Ok(ntr)
    }

    pub fn mem_read(&self, addr: u32, size: u32, pid: u32) -> Result<Vec<u8>, ()> {
        let mut ntr_stream = NtrStream::new(&self);
        try!(ntr_stream.send_mem_read_packet(addr, size, pid).map_err(|_x| ()));
        self.mem_read_rx.recv().map_err(|_x| ())
    }

    pub fn mem_write(&self, addr: u32, data: &Vec<u8>, pid: u32) -> io::Result<usize> {
        NtrStream::new(&self).send_mem_write_packet(addr, pid, data)
    }
}

#[derive(Debug)]
struct NtrStream<'a> {
    tcp_stream: TcpStream,
    current_seq: &'a AtomicUsize,
    is_heartbeat_sendable: &'a AtomicBool,
}
impl<'a> NtrStream<'a> {
    fn new(ntr: &'a Ntr) -> Self {
        NtrStream {
            tcp_stream: ntr.tcp_stream.try_clone().unwrap(),
            current_seq: &ntr.current_seq,
            is_heartbeat_sendable: &ntr.is_heartbeat_sendable,
        }
    }

    fn send_packet(&mut self, packet_type: u32, cmd: u32, args: &[u32], data_len: u32) -> io::Result<usize> {
        let mut buf = [0u8; 84];

        LittleEndian::write_u32(&mut buf[0..4], 0x12345678);
        LittleEndian::write_u32(&mut buf[4..8], self.current_seq.fetch_add(1000, Ordering::SeqCst) as u32);
        LittleEndian::write_u32(&mut buf[8..12], packet_type);
        LittleEndian::write_u32(&mut buf[12..16], cmd);
        for i in 0..16 {
            LittleEndian::write_u32(&mut buf[(4 * i + 16)..(4 * i + 20)], args[i]);
        }
        LittleEndian::write_u32(&mut buf[80..84], data_len);

        self.tcp_stream.write(&buf)
    }

    fn send_mem_read_packet(&mut self, addr: u32, size: u32, pid: u32) -> io::Result<usize> {
        self.send_empty_packet(9, pid, addr, size)
    }

    fn send_mem_write_packet(&mut self, addr: u32, pid: u32, buf: &Vec<u8>) -> io::Result<usize> {
        let args = &mut [0u32; 16];
        args[0] = pid;
        args[1] = addr;
        args[2] = buf.len() as u32;
        try!(self.send_packet(1, 10, args, args[2]));
        self.tcp_stream.write(buf)
    }

    fn send_heartbeat_packet(&mut self) -> io::Result<usize> {
        self.send_packet(0, 0, &[0u32; 16], 0)
    }

    fn send_hello_packet(&mut self) -> io::Result<usize> {
        self.send_packet(0, 3, &[0u32; 16], 0)
    }

    fn send_reload_packet(&mut self) -> io::Result<usize> {
        self.send_packet(0, 4, &[0u32; 16], 0)
    }

    fn send_empty_packet(&mut self, cmd: u32, arg0: u32, arg1: u32, arg2: u32) -> io::Result<usize> {
        let mut args = [0u32; 16];
        args[0] = arg0;
        args[1] = arg1;
        args[2] = arg2;
        self.send_packet(0, cmd, &args, 0)
    }

    fn tcp_stream(&mut self) -> &TcpStream {
        &self.tcp_stream
    }

    fn is_heartbeat_sendable(&self) -> &AtomicBool {
        &self.is_heartbeat_sendable
    }
}