//! CPU Sets for a game under Game Mode: which logical processors its threads use by default.
//! The selection is a pure function over the topology Windows reports
//! (`GetSystemCpuSetInformation`), so it is tested on made-up CPUs; Game Mode applies it with
//! `SetProcessDefaultCpuSets` and clears it when the game closes. Never an empty set: when the
//! options would leave nothing (or change nothing), the game is left alone.

use std::collections::{BTreeMap, BTreeSet};
use std::mem::size_of;

use serde::Serialize;
use windows::Win32::System::SystemInformation::{CpuSetInformation, GetSystemCpuSetInformation, SYSTEM_CPU_SET_INFORMATION};

use crate::gamemode::GameTweaks;

/// One logical processor as a CPU Set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cpu {
    /// CPU Set id (what `SetProcessDefaultCpuSets` takes).
    pub id: u32,
    pub group: u16,
    /// Group-relative physical core.
    pub core: u8,
    /// Group-relative logical processor.
    pub logical: u8,
    /// Higher = faster core (P-cores on hybrid CPUs); all equal elsewhere.
    pub class: u8,
}

/// What the per-game options can do on this PC.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct Summary {
    pub logical: u32,
    pub physical: u32,
    /// Two or more logical processors share a core (Hyper-Threading / SMT).
    pub smt: bool,
    /// Cores of different efficiency classes (Intel P + E cores).
    pub hybrid: bool,
    /// Logical processors on the fastest cores.
    pub perf_logical: u32,
}

fn cores(cpus: &[Cpu]) -> BTreeMap<(u16, u8), Vec<&Cpu>> {
    let mut m: BTreeMap<(u16, u8), Vec<&Cpu>> = BTreeMap::new();
    for c in cpus {
        m.entry((c.group, c.core)).or_default().push(c);
    }
    m
}

pub fn summary(cpus: &[Cpu]) -> Summary {
    let cores = cores(cpus);
    let top = cpus.iter().map(|c| c.class).max().unwrap_or(0);
    Summary {
        logical: cpus.len() as u32,
        physical: cores.len() as u32,
        smt: cores.values().any(|v| v.len() > 1),
        hybrid: cpus.iter().map(|c| c.class).collect::<BTreeSet<_>>().len() > 1,
        perf_logical: cpus.iter().filter(|c| c.class == top).count() as u32,
    }
}

/// The CPU Set ids a game should use, or `None` = leave it alone (no option on, nothing would
/// change, or nothing would be left). Options combine as an intersection:
/// - `free_core0`: not the physical core of logical processor 0 (nor its SMT sibling), where
///   Windows routes most interrupts and DPCs;
/// - `physical_only`: one logical processor per physical core (no SMT sibling);
/// - `perf_cores_only`: only the highest efficiency class (P-cores on hybrid CPUs).
///
/// An option that does not apply to this CPU (no SMT, not hybrid) keeps everything.
pub fn select(cpus: &[Cpu], t: &GameTweaks) -> Option<Vec<u32>> {
    if !(t.free_core0 || t.physical_only || t.perf_cores_only) || cpus.is_empty() {
        return None;
    }
    let cores = cores(cpus);
    let first = cpus.iter().min_by_key(|c| (c.group, c.logical)).map(|c| (c.group, c.core));
    let top = cpus.iter().map(|c| c.class).max().unwrap_or(0);
    let ids: Vec<u32> = cpus
        .iter()
        .filter(|c| !t.free_core0 || Some((c.group, c.core)) != first)
        .filter(|c| {
            !t.physical_only || cores[&(c.group, c.core)].iter().map(|x| x.logical).min() == Some(c.logical)
        })
        .filter(|c| !t.perf_cores_only || c.class == top)
        .map(|c| c.id)
        .collect();
    (!ids.is_empty() && ids.len() < cpus.len()).then_some(ids)
}

/// This PC's logical processors (empty if Windows does not answer).
pub fn topology() -> Vec<Cpu> {
    let mut len = 0u32;
    // SAFETY: size probe with no buffer; fails with ERROR_INSUFFICIENT_BUFFER and sets `len`.
    let _ = unsafe { GetSystemCpuSetInformation(None, 0, &mut len, None, None) };
    if len == 0 {
        return Vec::new();
    }
    // u64 storage keeps every entry 8-byte aligned.
    let mut buf = vec![0u64; (len as usize).div_ceil(8)];
    let ptr = buf.as_mut_ptr().cast::<SYSTEM_CPU_SET_INFORMATION>();
    // SAFETY: the buffer holds `len` bytes.
    if !unsafe { GetSystemCpuSetInformation(Some(ptr), len, &mut len, None, None) }.as_bool() {
        return Vec::new();
    }
    let bytes = buf.as_ptr().cast::<u8>();
    let (mut off, mut out) = (0usize, Vec::new());
    while off + size_of::<SYSTEM_CPU_SET_INFORMATION>() <= len as usize {
        // SAFETY: in bounds (checked above); entries are variable-size, each starts with `Size`.
        let e = unsafe { &*bytes.add(off).cast::<SYSTEM_CPU_SET_INFORMATION>() };
        if e.Size == 0 {
            break;
        }
        if e.Type == CpuSetInformation {
            // SAFETY: `Type` says the union holds a CpuSet.
            let c = unsafe { e.Anonymous.CpuSet };
            out.push(Cpu { id: c.Id, group: c.Group, core: c.CoreIndex, logical: c.LogicalProcessorIndex, class: c.EfficiencyClass });
        }
        off += e.Size as usize;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `cores` = (efficiency class, threads per core), in order; ids start at 256 like Windows'.
    fn cpu(cores: &[(u8, u8)]) -> Vec<Cpu> {
        let mut out = Vec::new();
        for (core, (class, threads)) in cores.iter().enumerate() {
            for _ in 0..*threads {
                let logical = out.len() as u8;
                out.push(Cpu { id: 256 + u32::from(logical), group: 0, core: core as u8, logical, class: *class });
            }
        }
        out
    }

    fn opts(free_core0: bool, physical_only: bool, perf_cores_only: bool) -> GameTweaks {
        GameTweaks { free_core0, physical_only, perf_cores_only, ..Default::default() }
    }

    #[test]
    fn hybrid_8p_smt_16e() {
        let t = cpu(&[[(1u8, 2u8); 8].as_slice(), [(0u8, 1u8); 16].as_slice()].concat());
        let s = summary(&t);
        assert_eq!((s.logical, s.physical, s.smt, s.hybrid, s.perf_logical), (32, 24, true, true, 16));
        let perf = select(&t, &opts(false, false, true)).unwrap();
        assert_eq!(perf, (256..272).collect::<Vec<_>>(), "P-cores with their SMT siblings");
        let phys = select(&t, &opts(false, true, false)).unwrap();
        assert_eq!(phys.len(), 24, "one per core: 8 P + 16 E");
        let perf_phys = select(&t, &opts(false, true, true)).unwrap();
        assert_eq!(perf_phys, vec![256, 258, 260, 262, 264, 266, 268, 270]);
        let all3 = select(&t, &opts(true, true, true)).unwrap();
        assert_eq!(all3, vec![258, 260, 262, 264, 266, 268, 270], "core 0 and its sibling out");
        let free = select(&t, &opts(true, false, false)).unwrap();
        assert_eq!(free.len(), 30);
        assert!(!free.contains(&256) && !free.contains(&257));
    }

    #[test]
    fn plain_8c_16t() {
        let t = cpu(&[(0, 2); 8]);
        let s = summary(&t);
        assert_eq!((s.physical, s.smt, s.hybrid), (8, true, false));
        assert_eq!(select(&t, &opts(false, true, false)).unwrap().len(), 8);
        assert_eq!(select(&t, &opts(true, true, false)).unwrap().len(), 7);
        assert_eq!(select(&t, &opts(false, false, true)), None, "not hybrid: nothing to change");
    }

    #[test]
    fn four_cores_no_smt() {
        let t = cpu(&[(0, 1); 4]);
        assert!(!summary(&t).smt);
        assert_eq!(select(&t, &opts(false, true, false)), None, "no SMT: no change");
        assert_eq!(select(&t, &opts(true, false, false)), Some(vec![257, 258, 259]));
        assert_eq!(select(&t, &opts(true, true, true)), Some(vec![257, 258, 259]));
    }

    #[test]
    fn single_core_never_empties() {
        let t = cpu(&[(0, 1)]);
        assert_eq!(select(&t, &opts(true, true, true)), None);
        let smt1 = cpu(&[(0, 2)]);
        assert_eq!(select(&smt1, &opts(true, false, false)), None, "the only core");
        assert_eq!(select(&smt1, &opts(false, true, false)), Some(vec![256]));
        assert_eq!(select(&[], &opts(true, true, true)), None);
        assert_eq!(select(&t, &opts(false, false, false)), None, "no option on");
    }

    #[test]
    fn second_group_core_numbers_do_not_collide() {
        // Two processor groups, each with its own core 0.
        let mut t = cpu(&[(0, 2); 2]);
        t.extend(cpu(&[(0, 2); 2]).into_iter().map(|c| Cpu { id: c.id + 100, group: 1, ..c }));
        assert_eq!(summary(&t).physical, 4);
        let free = select(&t, &opts(true, false, false)).unwrap();
        assert_eq!(free, vec![258, 259, 356, 357, 358, 359], "only group 0's core 0 is freed");
    }

    #[test]
    fn live_topology_is_consistent() {
        let t = topology();
        assert!(!t.is_empty());
        let s = summary(&t);
        assert!(s.physical >= 1 && s.physical <= s.logical && s.perf_logical >= 1);
        let ids: BTreeSet<u32> = t.iter().map(|c| c.id).collect();
        assert_eq!(ids.len(), t.len(), "unique CPU Set ids");
    }
}
