use super::{n16, n32, Checksum};
use std::{mem, slice, u8};

use ip::Ipv4Addr;

pub const TCP_FIN: u16 = 1;
pub const TCP_SYN: u16 = 1 << 1;
pub const TCP_RST: u16 = 1 << 2;
pub const TCP_PSH: u16 = 1 << 3;
pub const TCP_ACK: u16 = 1 << 4;

#[derive(Copy, Clone, Debug)]
#[repr(packed)]
pub struct TcpHeader {
    pub src: n16,
    pub dst: n16,
    pub sequence: n32,
    pub ack_num: n32,
    pub flags: n16,
    pub window_size: n16,
    pub checksum: Checksum,
    pub urgent_pointer: n16,
}

#[derive(Clone, Debug)]
pub struct Tcp {
    pub header: TcpHeader,
    pub options: Vec<u8>,
    pub data: Vec<u8>,
}

impl Tcp {
    pub fn checksum(&mut self, src_addr: &Ipv4Addr, dst_addr: &Ipv4Addr) {
        self.header.checksum.data = 0;

        let proto = n16::new(0x06);
        let segment_len = n16::new((mem::size_of::<TcpHeader>() + self.options.len() + self.data.len()) as u16);
        self.header.checksum.data = Checksum::compile(unsafe {
            Checksum::sum(src_addr.bytes.as_ptr() as usize, src_addr.bytes.len()) +
            Checksum::sum(dst_addr.bytes.as_ptr() as usize, dst_addr.bytes.len()) +
            Checksum::sum((&segment_len as *const n16) as usize, mem::size_of::<n16>()) +
            Checksum::sum((&proto as *const n16) as usize, mem::size_of::<n16>()) +
            Checksum::sum((&self.header as *const TcpHeader) as usize, mem::size_of::<TcpHeader>()) +
            Checksum::sum(self.options.as_ptr() as usize, self.options.len()) +
            Checksum::sum(self.data.as_ptr() as usize, self.data.len())
        });
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() >= mem::size_of::<TcpHeader>() {
            unsafe {
                let header = *(bytes.as_ptr() as *const TcpHeader);
                let header_len = ((header.flags.get() & 0xF000) >> 10) as usize;

                if header_len >= mem::size_of::<TcpHeader>() && header_len <= bytes.len() {
                    return Some(Tcp {
                        header: header,
                        options: bytes[mem::size_of::<TcpHeader>()..header_len].to_vec(),
                        data: bytes[header_len..bytes.len()].to_vec(),
                    });
                }
            }
        }
        None
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        unsafe {
            let header_ptr: *const TcpHeader = &self.header;
            let mut ret = Vec::from(slice::from_raw_parts(header_ptr as *const u8,
                                                          mem::size_of::<TcpHeader>()));
            ret.extend_from_slice(&self.options);
            ret.extend_from_slice(&self.data);
            ret
        }
    }
}
