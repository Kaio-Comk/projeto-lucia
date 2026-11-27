// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) Projeto Lucia contributors.
// Adapted from Kelvaroq patch 0063-uma (GPL-3.0-or-later) for base 52ac4ec.

use super::*;

pub(super) fn prepare_directory(
    host: &Host,
    submitted: &mut u64,
    runs: &[&HostList],
    bound: usize,
) -> crate::memory::pool::Reads {
    #[cfg(feature = "diagnostics")]
    let begin = std::time::Instant::now();
    host.settle(submitted, WHY_READ);
    #[cfg(feature = "diagnostics")]
    let settled = std::time::Instant::now();
    let writes = runs.iter().any(|list| {
        list.deferred()
            .copies
            .iter()
            .any(|step| matches!(step, Step::GpuWrite(..) | Step::Run(..) | Step::Copy(..)))
    });
    let reads = crate::memory::pool::prepare(if writes { 0 } else { bound });
    #[cfg(feature = "diagnostics")]
    crate::inplace::pool::note_preparation(settled - begin, settled.elapsed());
    reads
}
