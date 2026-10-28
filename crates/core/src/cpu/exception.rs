use crate::cpu::{Cpu, FLAG_I, FLAG_T, IRQ_STACK, MODE_MASK, Mode, SVC_STACK, USER_STACK};
use crate::error::{Error, Result};

impl Cpu {
    /// Set the GBA BIOS stack tops documented at `#gba-bios-ram-usage`.
    /// Direct-entry test state remains unchanged unless this is called.
    pub fn initialize_bios_stacks(&mut self) {
        self.sp[Mode::System.bank()] = USER_STACK;
        self.sp[Mode::Irq.bank()] = IRQ_STACK;
        self.sp[Mode::Svc.bank()] = SVC_STACK;
    }

    pub(crate) fn svc_stack_pointer(&self) -> u32 {
        self.sp[Mode::Svc.bank()]
    }

    pub(crate) fn set_svc_stack_pointer(&mut self, value: u32) {
        self.sp[Mode::Svc.bank()] = value;
    }

    /// True while the CPSR I bit masks IRQs.
    pub(crate) fn irq_masked(&self) -> bool {
        self.cpsr & FLAG_I != 0
    }

    /// ARM7TDMI IRQ entry. `LR_irq` receives the return address that the
    /// documented `SUBS PC,LR,#4` epilogue rewinds by one instruction, and the
    /// handler address is the pointer the guest wrote to `0x03007FFC`.
    pub(crate) fn enter_irq(&mut self, handler: u32) -> u32 {
        let old_cpsr = self.cpsr;
        self.spsr[Mode::Irq.bank()] = Some(old_cpsr);
        self.lr[Mode::Irq.bank()] = self.pc.wrapping_add(4);
        // IRQ entry switches to ARM state with IRQs masked and FIQ unchanged.
        self.cpsr = (old_cpsr & !(MODE_MASK | FLAG_T)) | Mode::Irq.bits() | FLAG_I;
        self.pc = handler & !3;
        old_cpsr
    }

    /// ARM7TDMI SWI entry. The caller has already validated the service and
    /// rejected unsupported SVC-origin nesting.
    pub(crate) fn enter_swi(&mut self, resume_pc: u32) -> u32 {
        let old_cpsr = self.cpsr;
        self.spsr[Mode::Svc.bank()] = Some(old_cpsr);
        self.lr[Mode::Svc.bank()] = resume_pc;
        self.cpsr = (old_cpsr & !(MODE_MASK | FLAG_T)) | Mode::Svc.bits() | FLAG_I;
        self.pc = 0x0000_0008;
        old_cpsr
    }

    pub(crate) fn swi_handler_to_system(&mut self) {
        self.cpsr = (self.cpsr & !MODE_MASK) | Mode::System.bits();
    }

    pub(crate) fn swi_handler_to_svc(&mut self) {
        self.cpsr = (self.cpsr & !MODE_MASK) | Mode::Svc.bits();
    }

    pub(crate) fn save_swi_frame_pointer(&mut self, value: u32) {
        self.set_svc_stack_pointer(value);
    }

    /// Restore the documented SWI frame in SVC mode before `MOVS PC,LR`.
    pub(crate) fn restore_swi_frame(
        &mut self,
        stack_after_pop: u32,
        saved_spsr: u32,
        saved_r11: u32,
        saved_r12: u32,
        saved_lr: u32,
    ) {
        self.spsr[Mode::Svc.bank()] = Some(saved_spsr);
        self.set_reg(11, saved_r11);
        self.set_reg(12, saved_r12);
        self.lr[Mode::Svc.bank()] = saved_lr;
        self.set_svc_stack_pointer(stack_after_pop);
    }

    /// Architectural SWI return (`MOVS PC,R14_svc`): restore PC and the full
    /// interrupted CPSR, including T, mode, I/F and NZCV.
    pub(crate) fn return_from_swi(&mut self) -> Result<()> {
        let bank = Mode::Svc.bank();
        let status = self.spsr[bank].ok_or(Error::Psr("SPSR_svc is unavailable"))?;
        let resume_pc = self.lr[bank];
        if Mode::from_bits(status).is_none() {
            return Err(Error::Psr("SPSR_svc contains an invalid return mode"));
        }
        self.cpsr = status;
        self.pc = resume_pc;
        Ok(())
    }
}
