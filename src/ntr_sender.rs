
use std::io;
use std::io::prelude::*;
use std::net::TcpStream;
use byteorder::{ByteOrder, LittleEndian};

#[derive(Debug)]
pub struct NtrSender {
    tcp_stream: TcpStream,
    current_seq: u32,
    is_heartbeat_sendable: bool,
}
impl NtrSender {
    pub fn new(tcp_stream: TcpStream) -> Self {
        NtrSender {
            tcp_stream: tcp_stream,
            current_seq: 1000,
            is_heartbeat_sendable: true,
        }
    }

    pub fn is_heartbeat_sendable(&self) -> bool {
        self.is_heartbeat_sendable
    }

    pub fn set_is_heartbeat_sendable(&mut self, b: bool) {
        self.is_heartbeat_sendable = b;
    }

    pub fn send_mem_read_packet(&mut self, addr: u32, size: u32, pid: u32) -> io::Result<usize> {
        self.send_empty_packet(9, pid, addr, size)
    }

    pub fn send_mem_write_packet(&mut self, addr: u32, pid: u32, buf: &Vec<u8>) -> io::Result<usize> {
        let args = &mut [0u32; 16];
        args[0] = pid;
        args[1] = addr;
        args[2] = buf.len() as u32;
        try!(self.send_packet(1, 10, args, args[2]));
        self.tcp_stream.write(buf)
    }

    pub fn send_heartbeat_packet(&mut self) -> io::Result<usize> {
        self.send_packet(0, 0, &[0u32; 16], 0)
    }

    pub fn send_hello_packet(&mut self) -> io::Result<usize> {
        self.send_packet(0, 3, &[0u32; 16], 0)
    }

    pub fn send_reload_packet(&mut self) -> io::Result<usize> {
        self.send_packet(0, 4, &[0u32; 16], 0)
    }

    fn send_packet(&mut self, packet_type: u32, cmd: u32, args: &[u32], data_len: u32) -> io::Result<usize> {
        let mut buf = [0u8; 84];

        LittleEndian::write_u32(&mut buf[0..4], 0x12345678);
        LittleEndian::write_u32(&mut buf[4..8], self.current_seq);
        LittleEndian::write_u32(&mut buf[8..12], packet_type);
        LittleEndian::write_u32(&mut buf[12..16], cmd);
        for i in 0..16 {
            LittleEndian::write_u32(&mut buf[(4 * i + 16)..(4 * i + 20)], args[i]);
        }
        LittleEndian::write_u32(&mut buf[80..84], data_len);

        self.current_seq += 1000;
        self.tcp_stream.write(&buf)
    }

    fn send_empty_packet(&mut self, cmd: u32, arg0: u32, arg1: u32, arg2: u32) -> io::Result<usize> {
        let mut args = [0u32; 16];
        args[0] = arg0;
        args[1] = arg1;
        args[2] = arg2;
        self.send_packet(0, cmd, &args, 0)
    }
}