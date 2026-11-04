// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! The pcapng file every captured frame is recorded to: always in debug builds,
//! and beside the raw packet log while "Log raw packets" is on (the frames
//! before decryption, so a login whose key was never found can still be
//! replayed with `--replay-export`).

use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

/// pcapng option code for `if_name`.
const OPT_IF_NAME: u16 = 2;
/// pcapng option code for `if_tsresol` (interface timestamp resolution).
const OPT_IF_TSRESOL: u16 = 9;

/// `if_tsresol` value for nanosecond timestamps.
///
/// The high bit selects the base (0 = power of ten, 1 = power of two); the
/// remaining bits are the negative exponent. `9` therefore means 10^-9 s.
/// Without this option the pcapng default is 10^-6 s, so a reader would divide
/// our nanosecond timestamps by a million and place every frame in 1970.
const IF_TSRESOL_NANOSECONDS: u8 = 9;

pub struct PcapngWriter {
    file: File,
    packet_count: u32,
    /// The file's interfaces by id: interface 0 is the capture itself, and a
    /// backend that reports which point of the stack delivered a frame
    /// (pktmon's components) gets one per source, named after it, so a log
    /// shows where each frame was seen. A reader that ignores names reads
    /// every interface as Ethernet.
    sources: Vec<Option<u16>>,
}

impl PcapngWriter {
    pub fn new(path: PathBuf) -> std::io::Result<Self> {
        let file = File::create(path)?;
        let mut writer = PcapngWriter {
            file,
            packet_count: 0,
            sources: vec![None],
        };
        writer.write_shb()?;
        writer.write_idb(None)?;
        Ok(writer)
    }

    fn write_shb(&mut self) -> std::io::Result<()> {
        let mut block = Vec::new();
        block.extend_from_slice(&0x0a0d0d0a_u32.to_le_bytes());

        let total_len: u32 = 32;
        block.extend_from_slice(&total_len.to_le_bytes());

        block.extend_from_slice(&0x1a2b3c4d_u32.to_le_bytes());
        block.extend_from_slice(&1_u16.to_le_bytes());
        block.extend_from_slice(&0_u16.to_le_bytes());
        block.extend_from_slice(&0xffffffffffffffff_u64.to_le_bytes());

        block.extend_from_slice(&0_u32.to_le_bytes());

        block.extend_from_slice(&total_len.to_le_bytes());

        self.file.write_all(&block)
    }

    fn write_idb(&mut self, name: Option<&str>) -> std::io::Result<()> {
        self.file.write_all(&Self::idb_block(name))
    }

    /// Build an Interface Description Block, with an `if_name` when given.
    ///
    /// Layout: block type, block length, linktype + reserved, snaplen, the
    /// options (`if_name`, `if_tsresol`), `opt_endofopt`, and the trailing
    /// block length. Every option value is padded to 4 bytes.
    fn idb_block(name: Option<&str>) -> Vec<u8> {
        let mut block = Vec::new();
        block.extend_from_slice(&0x00000001_u32.to_le_bytes());

        // 4 (type) + 4 (len) + 4 (linktype/reserved) + 4 (snaplen)
        //   + the name option (4 + its padded length)
        //   + 8 (if_tsresol option, 1 byte of value padded to 4)
        //   + 4 (opt_endofopt) + 4 (trailing len)
        let name_len = name.map_or(0, |name| 4 + ((name.len() + 3) & !3));
        let total_len = (32 + name_len) as u32;
        block.extend_from_slice(&total_len.to_le_bytes());

        block.extend_from_slice(&1_u16.to_le_bytes());
        block.extend_from_slice(&0_u16.to_le_bytes());
        // snaplen 0 = no limit: a merged datagram from the raw-socket backend
        // is an IP packet of up to 65,535 bytes plus its synthetic Ethernet
        // header, more than the 65,536 this used to declare.
        block.extend_from_slice(&0_u32.to_le_bytes());

        if let Some(name) = name {
            block.extend_from_slice(&OPT_IF_NAME.to_le_bytes());
            block.extend_from_slice(&(name.len() as u16).to_le_bytes());
            block.extend_from_slice(name.as_bytes());
            block.resize(block.len() + (name_len - 4 - name.len()), 0);
        }

        // if_tsresol: our packet timestamps are nanoseconds since the epoch.
        block.extend_from_slice(&OPT_IF_TSRESOL.to_le_bytes());
        block.extend_from_slice(&1_u16.to_le_bytes());
        block.push(IF_TSRESOL_NANOSECONDS);
        block.extend_from_slice(&[0u8; 3]);

        // opt_endofopt (code 0, length 0).
        block.extend_from_slice(&0_u32.to_le_bytes());

        block.extend_from_slice(&total_len.to_le_bytes());

        block
    }

    /// The interface id for frames from `source`, describing it in the file
    /// the first time it is seen.
    fn interface_for(&mut self, source: Option<u16>) -> std::io::Result<u32> {
        if let Some(id) = self.sources.iter().position(|known| *known == source) {
            return Ok(id as u32);
        }
        let name = source.map(|component| format!("pktmon component {component}"));
        self.write_idb(name.as_deref())?;
        self.sources.push(source);
        Ok((self.sources.len() - 1) as u32)
    }

    /// Record a frame of the capture itself (interface 0).
    pub fn write_packet(&mut self, timestamp_ns: u64, data: &[u8]) -> std::io::Result<()> {
        self.write_packet_from(timestamp_ns, None, data)
    }

    /// Record a frame delivered by `source` (a pktmon component id; `None` for
    /// the capture itself).
    pub fn write_packet_from(
        &mut self,
        timestamp_ns: u64,
        source: Option<u16>,
        data: &[u8],
    ) -> std::io::Result<()> {
        let interface_id = self.interface_for(source)?;
        let mut block = Vec::new();

        block.extend_from_slice(&0x00000006_u32.to_le_bytes());

        let ts_high: u32 = (timestamp_ns >> 32) as u32;
        let ts_low: u32 = (timestamp_ns & 0xffffffff) as u32;
        let captured_len: u32 = data.len() as u32;
        let orig_len: u32 = data.len() as u32;

        let padded_len = (data.len() + 3) & !3usize;
        let options_end: u32 = 0;

        let total_len: u32 = 4 + 4 + 4 + 4 + 4 + 4 + 4 + padded_len as u32 + 4 + 4;

        block.extend_from_slice(&total_len.to_le_bytes());
        block.extend_from_slice(&interface_id.to_le_bytes());
        block.extend_from_slice(&ts_high.to_le_bytes());
        block.extend_from_slice(&ts_low.to_le_bytes());
        block.extend_from_slice(&captured_len.to_le_bytes());
        block.extend_from_slice(&orig_len.to_le_bytes());

        block.extend_from_slice(data);
        if padded_len > data.len() {
            block.extend(vec![0u8; padded_len - data.len()]);
        }

        block.extend_from_slice(&options_end.to_le_bytes());

        block.extend_from_slice(&total_len.to_le_bytes());

        self.file.write_all(&block)?;
        self.packet_count += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idb_declares_nanosecond_timestamps() {
        let block = PcapngWriter::idb_block(None);

        // The declared length must match both length fields and the real size.
        assert_eq!(block.len(), 32);
        let declared = u32::from_le_bytes(block[4..8].try_into().unwrap());
        let trailing = u32::from_le_bytes(block[28..32].try_into().unwrap());
        assert_eq!(declared, 32);
        assert_eq!(trailing, 32);
        assert_eq!(declared as usize, block.len());

        // Block type and linktype (LINKTYPE_ETHERNET) are unchanged.
        assert_eq!(u32::from_le_bytes(block[0..4].try_into().unwrap()), 1);
        assert_eq!(u16::from_le_bytes(block[8..10].try_into().unwrap()), 1);
        assert_eq!(u32::from_le_bytes(block[12..16].try_into().unwrap()), 0);

        // if_tsresol = 9 (10^-9 s) in a 1-byte value padded to 4 bytes.
        assert_eq!(u16::from_le_bytes(block[16..18].try_into().unwrap()), 9);
        assert_eq!(u16::from_le_bytes(block[18..20].try_into().unwrap()), 1);
        assert_eq!(block[20], 9);
        assert_eq!(&block[21..24], &[0, 0, 0]);

        // opt_endofopt terminates the option list.
        assert_eq!(u32::from_le_bytes(block[24..28].try_into().unwrap()), 0);
    }

    #[test]
    fn idb_block_length_is_a_multiple_of_four() {
        // pcapng requires every block to be 32-bit aligned.
        assert_eq!(PcapngWriter::idb_block(None).len() % 4, 0);
        for name in [
            "pktmon component 7",
            "pktmon component 13",
            "a",
            "ab",
            "abc",
            "abcd",
        ] {
            let block = PcapngWriter::idb_block(Some(name));
            assert_eq!(block.len() % 4, 0, "{name}");
            let declared = u32::from_le_bytes(block[4..8].try_into().unwrap()) as usize;
            assert_eq!(declared, block.len(), "{name}");
            assert_eq!(&block[block.len() - 4..], &block[4..8], "{name}");
        }
    }

    #[test]
    fn a_named_idb_carries_if_name_before_if_tsresol() {
        let block = PcapngWriter::idb_block(Some("pktmon component 13"));
        // if_name (code 2) with the name's exact length, padded to 4.
        assert_eq!(u16::from_le_bytes(block[16..18].try_into().unwrap()), 2);
        assert_eq!(u16::from_le_bytes(block[18..20].try_into().unwrap()), 19);
        assert_eq!(&block[20..39], b"pktmon component 13");
        assert_eq!(block[39], 0);
        // then if_tsresol = 9.
        assert_eq!(u16::from_le_bytes(block[40..42].try_into().unwrap()), 9);
        assert_eq!(block[44], 9);
    }

    #[test]
    fn frames_from_different_sources_get_their_own_interfaces() {
        let dir = std::env::temp_dir().join(format!("teyvatme-pcapng-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sources.pcapng");
        {
            let mut writer = PcapngWriter::new(path.clone()).unwrap();
            writer.write_packet_from(1, Some(13), &[1, 2, 3]).unwrap();
            writer.write_packet(2, &[4, 5, 6]).unwrap();
            writer.write_packet_from(3, Some(39), &[7]).unwrap();
            writer.write_packet_from(4, Some(13), &[8]).unwrap();
            assert_eq!(writer.sources, vec![None, Some(13), Some(39)]);
        }
        let bytes = std::fs::read(&path).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();

        // Walk the blocks: SHB, IDB 0, IDB 1, EPB(if 1), EPB(if 0), IDB 2,
        // EPB(if 2), EPB(if 1).
        let mut at = 0;
        let mut kinds = Vec::new();
        while at + 8 <= bytes.len() {
            let block_type = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
            let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
            kinds.push(match block_type {
                0x0a0d0d0a => "shb".to_string(),
                1 => "idb".to_string(),
                6 => format!(
                    "epb {}",
                    u32::from_le_bytes(bytes[at + 8..at + 12].try_into().unwrap())
                ),
                other => panic!("unexpected block {other:#x}"),
            });
            at += len;
        }
        assert_eq!(at, bytes.len());
        assert_eq!(
            kinds,
            [
                "shb", "idb", "idb", "epb 1", "epb 0", "idb", "epb 2", "epb 1"
            ]
        );
    }
}
