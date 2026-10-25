use std::collections::HashMap;

use crate::bus::Bus;

use crate::cpu::Cpu;

use crate::display;
use crate::error::{Error, Result};
use crate::sha256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeReport {
    pub pc: u32,
    pub cpsr: u32,
    pub thumb: bool,
    pub regs: [u32; 16],
    pub budget_remaining: u64,
    pub budget_used: u64,
    pub fallbacks: u64,
    /// Digest of all CPU state, including inactive register banks and SPSRs.
    pub cpu_sha256: String,
    pub rom_sha256: String,
    pub vram_sha256: String,
    pub bus_sha256: String,
    pub image_width: u32,
    pub image_height: u32,
    pub image_sha256: String,
}

impl NativeReport {
    pub fn parse(text: &str) -> Result<Self> {
        let mut fields = HashMap::new();
        for line in text.lines() {
            if let Some((key, value)) = line.split_once('=') {
                fields.insert(key.trim(), value.trim());
            }
        }
        if fields.get("status").copied() != Some("ok") {
            return Err(Error::Native("missing status=ok".to_string()));
        }
        let mut regs = [0u32; 16];
        for (index, register) in regs.iter_mut().enumerate() {
            *register = parse_hex(field(&fields, &format!("r{index}"))?)?;
        }
        Ok(Self {
            pc: parse_hex(field(&fields, "pc")?)?,
            cpsr: parse_hex(field(&fields, "cpsr")?)?,
            thumb: match field(&fields, "thumb")? {
                "1" => true,
                "0" => false,
                _ => return Err(Error::Native("invalid thumb field".to_string())),
            },
            regs,
            budget_remaining: parse_u64(field(&fields, "budget_remaining")?)?,
            budget_used: parse_u64(field(&fields, "budget_used")?)?,
            fallbacks: parse_u64(field(&fields, "fallbacks")?)?,
            cpu_sha256: field(&fields, "cpu_sha256")?.to_string(),
            rom_sha256: field(&fields, "rom_sha256")?.to_string(),
            vram_sha256: field(&fields, "vram_sha256")?.to_string(),
            bus_sha256: field(&fields, "bus_sha256")?.to_string(),
            image_width: parse_u64(field(&fields, "image_width")?)? as u32,
            image_height: parse_u64(field(&fields, "image_height")?)? as u32,
            image_sha256: field(&fields, "image_sha256")?.to_string(),
        })
    }

    pub fn matches_reference(&self, cpu: &Cpu, bus: &Bus, budget_used: u64) -> Result<bool> {
        let mut regs = [0u32; 16];
        for (index, register) in regs.iter_mut().enumerate() {
            *register = cpu.reg(index as u8);
        }
        let image = display::snapshot(bus)?;
        Ok(self.pc == cpu.pc()
            && self.cpsr == cpu.cpsr()
            && self.thumb == cpu.thumb()
            && self.regs == regs
            && self.budget_used == budget_used
            && self.cpu_sha256 == sha256::hex(&cpu.state_digest())
            && self.rom_sha256 == bus.cart().sha256_hex()
            && self.vram_sha256 == sha256::hex(&bus.vram_digest())
            && self.bus_sha256 == sha256::hex(&bus.state_digest())
            && self.image_width == image.width
            && self.image_height == image.height
            && self.image_sha256 == image.sha256_hex())
    }
}

fn field<'a>(fields: &'a std::collections::HashMap<&str, &str>, name: &str) -> Result<&'a str> {
    fields
        .get(name)
        .copied()
        .ok_or_else(|| Error::Native(format!("missing field {name}")))
}

fn parse_hex(value: &str) -> Result<u32> {
    let value = value.strip_prefix("0x").unwrap_or(value);
    u32::from_str_radix(value, 16).map_err(|_| Error::Native(format!("invalid hex {value:?}")))
}

fn parse_u64(value: &str) -> Result<u64> {
    value
        .parse()
        .map_err(|_| Error::Native(format!("invalid integer {value:?}")))
}
