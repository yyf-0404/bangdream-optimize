//! Complete, streaming five-character enumeration in an explicitly supplied pool.
//! No fixed-timeline contribution/dominance pruning is valid for this new target.
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug)]
pub struct TimingCardIdentity {
    pub card_id: u32,
    pub character_id: u32,
}

#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnumerationStats {
    pub visited_branches: u64,
    pub character_conflicts: u64,
    pub teams: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum EnumerationError<E> {
    #[error("duplicate card ID in timing candidate pool")]
    DuplicateCard,
    #[error("timing enumeration exceeded the explicit team limit {limit}; incomplete search")]
    Limit { limit: usize },
    #[error("candidate callback failed: {0}")]
    Callback(E),
}

/// Emits input indices, once per unordered roster. A limit is an ERROR rather
/// than an apparently complete truncated optimum. No random-bucket fallback.
pub fn enumerate<E>(
    cards: &[TimingCardIdentity],
    limit: usize,
    mut visit: impl FnMut([usize; 5]) -> Result<(), E>,
) -> Result<EnumerationStats, EnumerationError<E>> {
    if cards
        .iter()
        .map(|c| c.card_id)
        .collect::<BTreeSet<_>>()
        .len()
        != cards.len()
    {
        return Err(EnumerationError::DuplicateCard);
    }
    fn walk<E>(
        cards: &[TimingCardIdentity],
        start: usize,
        depth: usize,
        team: &mut [usize; 5],
        limit: usize,
        stats: &mut EnumerationStats,
        visit: &mut impl FnMut([usize; 5]) -> Result<(), E>,
    ) -> Result<(), EnumerationError<E>> {
        if depth == 5 {
            if stats.teams == limit {
                return Err(EnumerationError::Limit { limit });
            }
            visit(*team).map_err(EnumerationError::Callback)?;
            stats.teams += 1;
            return Ok(());
        }
        if cards.len() - start < 5 - depth {
            return Ok(());
        }
        for i in start..=cards.len() - (5 - depth) {
            stats.visited_branches += 1;
            if team[..depth]
                .iter()
                .any(|&j| cards[j].character_id == cards[i].character_id)
            {
                stats.character_conflicts += 1;
                continue;
            }
            team[depth] = i;
            walk(cards, i + 1, depth + 1, team, limit, stats, visit)?;
        }
        Ok(())
    }
    let mut stats = EnumerationStats::default();
    walk(cards, 0, 0, &mut [0; 5], limit, &mut stats, &mut visit)?;
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_rosters_and_explicit_failure_on_truncation() {
        let cards: Vec<_> = (0..10)
            .map(|i| TimingCardIdentity {
                card_id: i,
                character_id: i / 2,
            })
            .collect();
        let mut found = BTreeSet::new();
        let stats = enumerate(&cards, 32, |t| {
            assert!(found.insert(t));
            Ok::<_, ()>(())
        })
        .unwrap();
        let mut expected = BTreeSet::new();
        for bits in 0..32 {
            expected.insert(std::array::from_fn(|i| i * 2 + ((bits >> i) & 1)));
        }
        assert_eq!(found, expected);
        assert_eq!(stats.teams, 32);
        assert!(matches!(
            enumerate(&cards, 31, |_| Ok::<_, ()>(())),
            Err(EnumerationError::Limit { .. })
        ));
        assert_eq!(
            enumerate(&cards[..4], 0, |_| Ok::<_, ()>(()))
                .unwrap()
                .teams,
            0
        );
        let mut duplicate = cards.clone();
        duplicate[1].card_id = duplicate[0].card_id;
        assert!(matches!(
            enumerate(&duplicate, 100, |_| Ok::<_, ()>(())),
            Err(EnumerationError::DuplicateCard)
        ));
        assert!(matches!(
            enumerate(&cards, 100, |_| Err("test")),
            Err(EnumerationError::Callback("test"))
        ));
    }
}
