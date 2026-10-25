//! Typed SWI wrapper: documented SVC exception state and framed HLE dispatch.

use crate::bus::Bus;
use crate::cpu::{Cpu, Mode};
use crate::error::{Error, Result};

const FRAME_BYTES: u32 = 16;

/// Execute the documented GBA SWI wrapper for a typed ARM/Thumb comment.
/// Unknown services and service precondition failures are rejected before the
/// CPU or bus is changed. HLE is atomic with respect to guest instruction
/// scheduling; a service's internal work consumes no additional budget/timing.
pub fn execute_swi(
    comment: u32,
    thumb: bool,
    addr: u32,
    cpu: &mut Cpu,
    bus: &mut Bus,
) -> Result<()> {
    let service = if thumb {
        comment as u8
    } else {
        (comment >> 16) as u8
    };
    super::validate_call(service, cpu, bus)?;
    if cpu.mode() == Mode::Svc {
        return Err(Error::SwiNesting);
    }

    let svc_sp = cpu.svc_stack_pointer();
    if svc_sp & 3 != 0 {
        return Err(Error::SwiStack {
            sp: svc_sp,
            reason: "stack pointer is not word-aligned",
        });
    }
    let frame_sp = svc_sp.checked_sub(FRAME_BYTES).ok_or(Error::SwiStack {
        sp: svc_sp,
        reason: "four-word frame underflows the address space",
    })?;
    if !is_work_ram_frame(frame_sp) {
        return Err(Error::SwiStack {
            sp: svc_sp,
            reason: "four-word frame must fit wholly in EWRAM or IWRAM",
        });
    }

    let resume_pc = addr.wrapping_add(if thumb { 2 } else { 4 });
    let saved_cpsr = cpu.enter_swi(resume_pc);
    cpu.save_swi_frame_pointer(frame_sp);
    // GBATEK lists the frame values as SPSR, R11, R12, R14. This HLE model
    // places that tuple at ascending addresses from the decremented SVC SP.
    for (index, value) in [saved_cpsr, cpu.reg(11), cpu.reg(12), cpu.reg(14)]
        .into_iter()
        .enumerate()
    {
        bus.write32(frame_sp + index as u32 * 4, value)?;
    }

    cpu.swi_handler_to_system();
    super::call(service, cpu, bus)?;
    cpu.swi_handler_to_svc();

    let saved_spsr = bus.read32(frame_sp)?;
    let saved_r11 = bus.read32(frame_sp + 4)?;
    let saved_r12 = bus.read32(frame_sp + 8)?;
    let saved_lr = bus.read32(frame_sp + 12)?;
    cpu.restore_swi_frame(svc_sp, saved_spsr, saved_r11, saved_r12, saved_lr);
    cpu.return_from_swi()
}

fn is_work_ram_frame(start: u32) -> bool {
    Bus::is_work_ram_range(start, FRAME_BYTES as usize)
}
