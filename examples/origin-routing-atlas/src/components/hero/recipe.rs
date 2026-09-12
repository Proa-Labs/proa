// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Stable hooks for the hero section.

pub mod base {
    pub const CHIP_ACTIVE: &str =
        "border border-cyan text-cyan py-[4px] px-[10px] text-[10px] cursor-pointer no-underline";
    pub const CHIP: &str =
        "border border-line-mid text-ink3 py-[4px] px-[10px] text-[10px] cursor-pointer no-underline";
    pub const BLIPS: &[&str] = &[
        "absolute left-[72%] top-[28%] w-[10px] h-[10px] m-[-5px] rounded-full bg-cyan animate-[org-blip_3.4s_ease-out_0s_infinite]",
        "absolute left-[34%] top-[42%] w-[10px] h-[10px] m-[-5px] rounded-full bg-cyan animate-[org-blip_3.4s_ease-out_1.1s_infinite]",
        "absolute left-[58%] top-[66%] w-[10px] h-[10px] m-[-5px] rounded-full bg-cyan animate-[org-blip_3.4s_ease-out_2.2s_infinite]",
        "absolute left-[24%] top-[70%] w-[10px] h-[10px] m-[-5px] rounded-full bg-cyan animate-[org-blip_3.4s_ease-out_3.1s_infinite]",
        "absolute left-[80%] top-[55%] w-[10px] h-[10px] m-[-5px] rounded-full bg-cyan animate-[org-blip_3.4s_ease-out_4s_infinite]",
        "absolute left-[46%] top-[20%] w-[10px] h-[10px] m-[-5px] rounded-full bg-cyan animate-[org-blip_3.4s_ease-out_4.7s_infinite]",
        "absolute left-[66%] top-[82%] w-[10px] h-[10px] m-[-5px] rounded-full bg-cyan animate-[org-blip_3.4s_ease-out_5.2s_infinite]",
    ];
}
