// Copyright The pipewire-rs Contributors.
// SPDX-License-Identifier: MIT

//! A pass-through mono DSP filter using `pw_filter`.

use pipewire as pw;
use pw::{
    filter::{FilterListenerRc, FilterPortRc},
    properties::properties,
    spa,
};

struct Ports {
    input: FilterPortRc,
    output: FilterPortRc,
}

fn main() -> Result<(), pw::Error> {
    pw::init();

    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;
    let _filter = build_filter(core)?;
    mainloop.run();
    Ok(())
}

fn build_filter(core: pw::core::CoreRc) -> Result<FilterListenerRc<'static, Ports>, pw::Error> {
    let filter = pw::filter::FilterRc::new(
        core,
        "rust-filter",
        properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Filter",
            *pw::keys::MEDIA_ROLE => "DSP",
            *pw::keys::NODE_PASSIVE => "true",
        },
    )?;

    let input = filter.add_port(
        spa::utils::Direction::Input,
        pw::filter::FilterPortFlags::MAP_BUFFERS,
        properties! {
            *pw::keys::FORMAT_DSP => "32 bit float mono audio",
            *pw::keys::PORT_NAME => "input",
        },
        &mut [],
    )?;
    let output = filter.add_port(
        spa::utils::Direction::Output,
        pw::filter::FilterPortFlags::MAP_BUFFERS,
        properties! {
            *pw::keys::FORMAT_DSP => "32 bit float mono audio",
            *pw::keys::PORT_NAME => "output",
        },
        &mut [],
    )?;

    let listener = filter
        .add_local_listener_with_user_data(Ports { input, output })
        .process(|_, ports, position| {
            let Some(position) = position else {
                return;
            };
            let Ok(samples) = u32::try_from(position.clock.duration) else {
                return;
            };
            unsafe {
                let input = ports.input.dsp_buffer(samples).cast::<f32>();
                let output = ports.output.dsp_buffer(samples).cast::<f32>();
                if !input.is_null() && !output.is_null() {
                    std::ptr::copy_nonoverlapping(input, output, samples as usize);
                }
            }
        })
        .register()?;

    filter.connect(pw::filter::FilterFlags::RT_PROCESS, &mut [])?;
    Ok(listener)
}
