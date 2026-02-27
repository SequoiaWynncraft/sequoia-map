use std::collections::{HashMap, HashSet, VecDeque};
use std::ops::Range;

/// Damage ranges per tower damage upgrade level (0–11).
pub const DAMAGES: [Range<f64>; 12] = [
    1000.0..1500.0,
    1400.0..2100.0,
    1800.0..2700.0,
    2200.0..3300.0,
    2600.0..3900.0,
    3000.0..4500.0,
    3400.0..5100.0,
    3800.0..5700.0,
    4200.0..6300.0,
    4600.0..6900.0,
    5000.0..7500.0,
    5400.0..8100.0,
];

/// Attack speed multiplier per tower attack upgrade level (0–11).
pub const ATTACK_RATES: [f64; 12] = [
    0.5, 0.75, 1.0, 1.25, 1.61, 2.0, 2.5, 3.0, 3.1, 4.2, 4.35, 4.7,
];

/// Tower HP per health upgrade level (0–11).
pub const HEALTHS: [f64; 12] = [
    300_000.0,
    450_000.0,
    600_000.0,
    750_000.0,
    960_000.0,
    1_200_000.0,
    1_500_000.0,
    1_800_000.0,
    2_160_000.0,
    2_280_000.0,
    2_580_000.0,
    2_820_000.0,
];

/// Damage reduction percentage per defense upgrade level (0–11).
pub const DEFENSES: [f64; 12] = [
    10.0, 40.0, 55.0, 62.5, 70.0, 75.0, 79.0, 82.0, 84.0, 86.0, 88.0, 90.0,
];

/// Aura cooldown labels (level 0 = off, 1–3 = cooldown seconds).
pub const AURA_LABELS: [&str; 4] = ["Off", "24s", "18s", "12s"];

/// Volley cooldown labels (level 0 = off, 1–3 = cooldown seconds).
pub const VOLLEY_LABELS: [&str; 4] = ["Off", "20s", "15s", "10s"];

/// Per-connected-territory bonus multiplier (30% per connection).
const CONNECTION_BONUS: f64 = 0.30;

/// HQ external base multiplier (+50% before any externals).
const HQ_EXTERNAL_BASE: f64 = 1.5;

/// Per-external multiplier for HQ (+25% per external).
const HQ_EXTERNAL_BONUS: f64 = 0.25;

/// Apply connection and HQ multipliers to a base stat value.
///
/// `connections` = number of allied-owned connected territories.
/// `externals` = number of external connections (BFS within 3 hops).
pub fn calc_stat(base: f64, is_hq: bool, connections: u32, externals: u32) -> f64 {
    let conn_mult = 1.0 + (connections as f64 * CONNECTION_BONUS);
    if is_hq {
        let ext_mult = HQ_EXTERNAL_BASE + (externals as f64 * HQ_EXTERNAL_BONUS);
        base * conn_mult * ext_mult
    } else {
        base * conn_mult
    }
}

/// Compute average DPS for given damage level & attack rate level with multipliers.
pub fn calc_dps(
    damage_level: usize,
    attack_level: usize,
    is_hq: bool,
    connections: u32,
    externals: u32,
) -> f64 {
    let dmg = &DAMAGES[damage_level.min(11)];
    let avg_dmg = (dmg.start + dmg.end) / 2.0;
    let rate = ATTACK_RATES[attack_level.min(11)];
    let base_dps = avg_dmg * rate;
    calc_stat(base_dps, is_hq, connections, externals)
}

/// Compute effective HP for given health & defense levels with multipliers.
pub fn calc_ehp(
    health_level: usize,
    defense_level: usize,
    is_hq: bool,
    connections: u32,
    externals: u32,
) -> f64 {
    let hp = HEALTHS[health_level.min(11)];
    let def_pct = DEFENSES[defense_level.min(11)] / 100.0;
    let base_ehp = hp / (1.0 - def_pct);
    calc_stat(base_ehp, is_hq, connections, externals)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefenseRating {
    VeryLow,
    Low,
    Medium,
    High,
    VeryHigh,
}

impl DefenseRating {
    /// Determine rating from the sum of all 4 stat levels (damage + attack + health + defense).
    pub fn from_sum(stat_sum: u32) -> Self {
        match stat_sum {
            0..=8 => DefenseRating::VeryLow,
            9..=16 => DefenseRating::Low,
            17..=28 => DefenseRating::Medium,
            29..=38 => DefenseRating::High,
            _ => DefenseRating::VeryHigh,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            DefenseRating::VeryLow => "Very Low",
            DefenseRating::Low => "Low",
            DefenseRating::Medium => "Medium",
            DefenseRating::High => "High",
            DefenseRating::VeryHigh => "Very High",
        }
    }

    pub fn color_rgb(&self) -> (u8, u8, u8) {
        match self {
            DefenseRating::VeryLow => (140, 140, 140),
            DefenseRating::Low => (235, 87, 87),
            DefenseRating::Medium => (245, 197, 66),
            DefenseRating::High => (102, 204, 102),
            DefenseRating::VeryHigh => (80, 200, 220),
        }
    }
}

/// BFS up to `max_hops` from `start` through the connection graph.
/// Returns the set of territory names reachable (excluding `start` itself).
pub fn find_externals(
    start: &str,
    connections_map: &HashMap<String, Vec<String>>,
    max_hops: u32,
) -> HashSet<String> {
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    visited.insert(start.to_string());
    queue.push_back((start.to_string(), 0u32));

    while let Some((current, depth)) = queue.pop_front() {
        if depth >= max_hops {
            continue;
        }
        if let Some(neighbors) = connections_map.get(&current) {
            for neighbor in neighbors {
                if visited.insert(neighbor.clone()) {
                    queue.push_back((neighbor.clone(), depth + 1));
                }
            }
        }
    }

    visited.remove(start);
    visited
}

/// Count guild-owned connections and compute externals via BFS (max 3 hops
/// through same-guild chains, including direct connections).
///
/// `territory_name` — the selected territory.
/// `territory_connections` — its direct connection list.
/// `guild_uuid` — the owning guild's UUID.
/// `lookup` — given a territory name, returns `(guild_uuid, connections)` if it exists.
///
/// Returns `(guild_connections, total_connections, externals)`.
pub fn count_guild_connections<'a>(
    territory_name: &str,
    territory_connections: &'a [String],
    guild_uuid: &str,
    lookup: impl Fn(&str) -> Option<(&'a str, &'a [String])>,
) -> (u32, u32, u32) {
    let total_conn = territory_connections.len() as u32;

    // Count direct connections owned by same guild
    let guild_conn = territory_connections
        .iter()
        .filter(|conn| lookup(conn).is_some_and(|(uuid, _)| uuid == guild_uuid))
        .count() as u32;

    // BFS through same-guild territories (max 3 hops) to find externals
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    visited.insert(territory_name.to_string());
    queue.push_back((territory_name.to_string(), 0u32));

    while let Some((current, depth)) = queue.pop_front() {
        if depth >= 3 {
            continue;
        }
        let conns: &[String] = if current == territory_name {
            territory_connections
        } else if let Some((_, c)) = lookup(&current) {
            c
        } else {
            continue;
        };
        for neighbor in conns {
            if !visited.contains(neighbor.as_str())
                && let Some((uuid, _)) = lookup(neighbor)
                && uuid == guild_uuid
            {
                visited.insert(neighbor.clone());
                queue.push_back((neighbor.clone(), depth + 1));
            }
        }
    }

    // Externals include depth-1 direct neighbors and same-guild nodes out to 3 hops.
    visited.remove(territory_name);
    let ext = visited.len() as u32;

    (guild_conn, total_conn, ext)
}

/// Format large numbers with k/M suffixes.
pub fn format_stat(val: f64) -> String {
    if val >= 1_000_000.0 {
        format!("{:.1}M", val / 1_000_000.0)
    } else if val >= 1_000.0 {
        format!("{:.0}k", val / 1_000.0)
    } else {
        format!("{:.0}", val)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DefenseRating, calc_dps, calc_ehp, calc_stat, count_guild_connections, find_externals,
        format_stat,
    };
    use std::collections::{HashMap, HashSet};

    fn assert_close(actual: f64, expected: f64) {
        let diff = (actual - expected).abs();
        assert!(
            diff < 1e-9,
            "expected {expected}, got {actual} (diff: {diff})"
        );
    }

    #[test]
    fn count_guild_connections_includes_direct_neighbors_in_externals() {
        let map = HashMap::from([
            (
                "A".to_string(),
                (
                    "guild-a".to_string(),
                    vec!["B".to_string(), "C".to_string(), "D".to_string()],
                ),
            ),
            (
                "B".to_string(),
                (
                    "guild-a".to_string(),
                    vec!["A".to_string(), "E".to_string()],
                ),
            ),
            (
                "C".to_string(),
                (
                    "guild-a".to_string(),
                    vec!["A".to_string(), "F".to_string()],
                ),
            ),
            (
                "D".to_string(),
                ("guild-b".to_string(), vec!["A".to_string()]),
            ),
            (
                "E".to_string(),
                ("guild-a".to_string(), vec!["B".to_string()]),
            ),
            (
                "F".to_string(),
                ("guild-a".to_string(), vec!["C".to_string()]),
            ),
        ]);

        let connections = map.get("A").expect("A exists").1.as_slice();
        let (guild_conn, total_conn, externals) =
            count_guild_connections("A", connections, "guild-a", |name| {
                map.get(name)
                    .map(|(guild, conns)| (guild.as_str(), conns.as_slice()))
            });

        assert_eq!(guild_conn, 2);
        assert_eq!(total_conn, 3);
        assert_eq!(externals, 4);
    }

    #[test]
    fn count_guild_connections_caps_externals_at_three_hops() {
        let map = HashMap::from([
            (
                "A".to_string(),
                ("guild-a".to_string(), vec!["B".to_string()]),
            ),
            (
                "B".to_string(),
                (
                    "guild-a".to_string(),
                    vec!["A".to_string(), "E".to_string()],
                ),
            ),
            (
                "E".to_string(),
                (
                    "guild-a".to_string(),
                    vec!["B".to_string(), "H".to_string()],
                ),
            ),
            (
                "H".to_string(),
                (
                    "guild-a".to_string(),
                    vec!["E".to_string(), "I".to_string()],
                ),
            ),
            (
                "I".to_string(),
                ("guild-a".to_string(), vec!["H".to_string()]),
            ),
        ]);

        let connections = map.get("A").expect("A exists").1.as_slice();
        let (guild_conn, total_conn, externals) =
            count_guild_connections("A", connections, "guild-a", |name| {
                map.get(name)
                    .map(|(guild, conns)| (guild.as_str(), conns.as_slice()))
            });

        assert_eq!(guild_conn, 1);
        assert_eq!(total_conn, 1);
        assert_eq!(externals, 3);
    }

    #[test]
    fn calc_stat_non_hq_uses_only_connection_multiplier() {
        let stat = calc_stat(1000.0, false, 4, 24);
        assert_close(stat, 2200.0);
    }

    #[test]
    fn calc_stat_hq_uses_external_and_connection_formula() {
        let stat = calc_stat(5400.0, true, 4, 24);
        // 5400 * (1 + 0.3*4) * (1.5 + 0.25*24) = 89100
        assert_close(stat, 89_100.0);
    }

    #[test]
    fn calc_dps_base_case() {
        let dps = calc_dps(0, 0, false, 0, 0);
        assert_close(dps, 625.0);
    }

    #[test]
    fn calc_ehp_base_case() {
        let ehp = calc_ehp(0, 0, false, 0, 0);
        assert_close(ehp, 300_000.0 / 0.9);
    }

    #[test]
    fn calc_dps_clamps_level_above_11() {
        let clamped = calc_dps(99, 42, false, 0, 0);
        let max_level = calc_dps(11, 11, false, 0, 0);
        assert_close(clamped, max_level);
    }

    #[test]
    fn defense_rating_from_sum_boundaries() {
        assert_eq!(DefenseRating::from_sum(0), DefenseRating::VeryLow);
        assert_eq!(DefenseRating::from_sum(8), DefenseRating::VeryLow);
        assert_eq!(DefenseRating::from_sum(9), DefenseRating::Low);
        assert_eq!(DefenseRating::from_sum(16), DefenseRating::Low);
        assert_eq!(DefenseRating::from_sum(17), DefenseRating::Medium);
        assert_eq!(DefenseRating::from_sum(28), DefenseRating::Medium);
        assert_eq!(DefenseRating::from_sum(29), DefenseRating::High);
        assert_eq!(DefenseRating::from_sum(38), DefenseRating::High);
        assert_eq!(DefenseRating::from_sum(39), DefenseRating::VeryHigh);
    }

    #[test]
    fn format_stat_millions_thousands_hundreds() {
        assert_eq!(format_stat(1_250_000.0), "1.2M");
        assert_eq!(format_stat(12_000.0), "12k");
        assert_eq!(format_stat(999.0), "999");
    }

    #[test]
    fn find_externals_respects_max_hops() {
        let graph = HashMap::from([
            ("A".to_string(), vec!["B".to_string()]),
            ("B".to_string(), vec!["A".to_string(), "C".to_string()]),
            ("C".to_string(), vec!["B".to_string(), "D".to_string()]),
            ("D".to_string(), vec!["C".to_string()]),
        ]);

        let externals = find_externals("A", &graph, 2);
        assert_eq!(externals, HashSet::from(["B".to_string(), "C".to_string()]));
    }

    #[test]
    fn find_externals_empty_graph() {
        let graph = HashMap::new();
        let externals = find_externals("A", &graph, 3);
        assert!(externals.is_empty());
    }
}
