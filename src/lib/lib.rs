#![feature(associated_consts)]

use std::fs::File;
use std::io::{Result, Read, Write};
use std::{mem, slice, u8, u16};

pub use ip::Ipv4Addr;
pub use mac::MacAddr;

mod ip;
mod mac;

pub fn getcfg(key: &str) -> Result<String> {
    let mut value = String::new();
    let mut file = File::open(&format!("/etc/net/{}", key))?;
    file.read_to_string(&mut value)?;
    Ok(value.trim().to_string())
}

pub fn setcfg(key: &str, value: &str) -> Result<()> {
    let mut file = File::create(&format!("/etc/net/{}", key))?;
    file.write(value.as_bytes())?;
    file.set_len(value.len() as u64)?;
    file.sync_all()?;
    Ok(())
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[allow(non_camel_case_types)]
#[repr(packed)]
pub struct n16(u16);

impl n16 {
    pub fn new(value: u16) -> Self {
        n16(value.to_be())
    }

    pub fn get(&self) -> u16 {
        u16::from_be(self.0)
    }

    pub fn set(&mut self, value: u16) {
        self.0 = value.to_be();
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[allow(non_camel_case_types)]
#[repr(packed)]
pub struct n32(u32);

impl n32 {
    pub fn new(value: u32) -> Self {
        n32(value.to_be())
    }

    pub fn get(&self) -> u32 {
        u32::from_be(self.0)
    }

    pub fn set(&mut self, value: u32) {
        self.0 = value.to_be();
    }
}

#[derive(Copy, Clone, Debug)]
pub struct Checksum {
    pub data: u16,
}

impl Checksum {
    pub unsafe fn sum(mut ptr: usize, mut len: usize) -> usize {
        let mut sum = 0;

        while len > 1 {
            sum += *(ptr as *const u16) as usize;
            len -= 2;
            ptr += 2;
        }

        if len > 0 {
            sum += *(ptr as *const u8) as usize;
        }

        sum
    }

    pub fn compile(mut sum: usize) -> u16 {
        while (sum >> 16) > 0 {
            sum = (sum & 0xFFFF) + (sum >> 16);
        }

        0xFFFF - (sum as u16)
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(packed)]
pub struct ArpHeader {
    pub htype: n16,
    pub ptype: n16,
    pub hlen: u8,
    pub plen: u8,
    pub oper: n16,
    pub src_mac: MacAddr,
    pub src_ip: Ipv4Addr,
    pub dst_mac: MacAddr,
    pub dst_ip: Ipv4Addr,
}

#[derive(Clone)]
pub struct Arp {
    pub header: ArpHeader,
    pub data: Vec<u8>,
}

impl Arp {
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() >= mem::size_of::<ArpHeader>() {
            unsafe {
                return Some(Arp {
                    header: *(bytes.as_ptr() as *const ArpHeader),
                    data: bytes[mem::size_of::<ArpHeader>() ..].to_vec(),
                });
            }
        }
        None
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        unsafe {
            let header_ptr: *const ArpHeader = &self.header;
            let mut ret = Vec::from(slice::from_raw_parts(header_ptr as *const u8,
                                                          mem::size_of::<ArpHeader>()));
            ret.extend_from_slice(&self.data);
            ret
        }
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(packed)]
pub struct EthernetIIHeader {
    pub dst: MacAddr,
    pub src: MacAddr,
    pub ethertype: n16,
}

#[derive(Clone)]
pub struct EthernetII {
    pub header: EthernetIIHeader,
    pub data: Vec<u8>,
}

impl EthernetII {
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() >= mem::size_of::<EthernetIIHeader>() {
            unsafe {
                return Some(EthernetII {
                    header: *(bytes.as_ptr() as *const EthernetIIHeader),
                    data: bytes[mem::size_of::<EthernetIIHeader>() ..].to_vec(),
                });
            }
        }
        None
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        unsafe {
            let header_ptr: *const EthernetIIHeader = &self.header;
            let mut ret = Vec::from(slice::from_raw_parts(header_ptr as *const u8,
                                                          mem::size_of::<EthernetIIHeader>()));
            ret.extend_from_slice(&self.data);
            ret
        }
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(packed)]
pub struct Ipv4Header {
    pub ver_hlen: u8,
    pub services: u8,
    pub len: n16,
    pub id: n16,
    pub flags_fragment: n16,
    pub ttl: u8,
    pub proto: u8,
    pub checksum: Checksum,
    pub src: Ipv4Addr,
    pub dst: Ipv4Addr,
}

#[derive(Clone)]
pub struct Ipv4 {
    pub header: Ipv4Header,
    pub options: Vec<u8>,
    pub data: Vec<u8>,
}

impl Ipv4 {
    pub fn checksum(&mut self) {
        self.header.checksum.data = 0;

        self.header.checksum.data = Checksum::compile(unsafe {
            Checksum::sum((&self.header as *const Ipv4Header) as usize, mem::size_of::<Ipv4Header>())
        });
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() >= mem::size_of::<Ipv4Header>() {
            unsafe {
                let header = *(bytes.as_ptr() as *const Ipv4Header);
                let header_len = ((header.ver_hlen & 0xF) << 2) as usize;

                if header_len >= mem::size_of::<Ipv4Header>() && header_len <= bytes.len()
                    && header.len.get() as usize <= bytes.len() && header_len <= header.len.get() as usize
                {
                    return Some(Ipv4 {
                        header: header,
                        options: bytes[mem::size_of::<Ipv4Header>() .. header_len].to_vec(),
                        data: bytes[header_len .. header.len.get() as usize].to_vec(),
                    });
                }
            }
        }
        None
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        unsafe {
            let header_ptr: *const Ipv4Header = &self.header;
            let mut ret = Vec::<u8>::from(slice::from_raw_parts(header_ptr as *const u8,
                                                                mem::size_of::<Ipv4Header>()));
            ret.extend_from_slice(&self.options);
            ret.extend_from_slice(&self.data);
            ret
        }
    }
}

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

#[derive(Clone)]
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

#[derive(Copy, Clone, Debug)]
#[repr(packed)]
pub struct UdpHeader {
    pub src: n16,
    pub dst: n16,
    pub len: n16,
    pub checksum: Checksum,
}

#[derive(Clone)]
pub struct Udp {
    pub header: UdpHeader,
    pub data: Vec<u8>,
}

impl Udp {
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() >= mem::size_of::<UdpHeader>() {
            unsafe {
                let header = *(bytes.as_ptr() as *const UdpHeader);

                if header.len.get() as usize <= bytes.len() && mem::size_of::<UdpHeader>() <= header.len.get() as usize {
                    return Some(Udp {
                        header: header,
                        data: bytes[mem::size_of::<UdpHeader>()..header.len.get() as usize].to_vec(),
                    });
                }
            }
        }
        None
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        unsafe {
            let header_ptr: *const UdpHeader = &self.header;
            let mut ret = Vec::from(slice::from_raw_parts(header_ptr as *const u8,
                                                          mem::size_of::<UdpHeader>()));
            ret.extend_from_slice(&self.data);
            ret
        }
    }
}
