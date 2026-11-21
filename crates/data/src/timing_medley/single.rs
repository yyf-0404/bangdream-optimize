//! Single-song highest score over the configured cards, equipment and legal hit frames.
use super::{
    engine_error, int_score, SkillCatalogue,
    product::{
        prepare_inventory, timing_result, timing_selections, timing_team_order, TimingMaximizeInput,
    },
    timing_memo::Memo,
};
use crate::{
    timing_chart::time_distance::{TimeDistanceChart, TimeDistanceScoring},
    timing_plan::simplify::restore_chart_positions,
    DataError,
};
use bangdream_optimize_core::{
    area_item_combinations, calculate_area_item_percent,
    hit_timing::{
        bounds::{
            PoolTimingChoice, PoolTimingUpper, TimingScoreUpper, TimingUpperCache,
            TimingUpperScratch,
        },
        compiled::{CompiledTimingCandidates, TimingCompileCache, TimingScoreScratch},
        layouts::{LayoutMaximum, TimingLayouts},
        ranges::TimingRangeRequest,
        replay, ReplaySkill, ScoreEffect,
    },
    scoring::{
        preparation::selected_equipment, DeckPower, MemberPower, ScoringPower, ARITHMETIC_REVISION,
    },
    timing::Instant,
    BuildResult, CalculationMetrics, HitTimingSongResult, LiveVariant, MaximizeError, PreparedCard,
    SelectedAreaItems, SongBuildResult,
};
use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet},
};

const MAX_SEARCH_NODES: u64 = 50_000_000;

struct ItemCase {
    items: SelectedAreaItems,
    powers: Vec<MemberPower>,
}

impl ItemCase {
    fn dominates(&self, other: &Self) -> bool {
        self.powers
            .iter()
            .zip(&other.powers)
            .all(|(a, b)| a.componentwise_dominates(*b))
    }
}

struct Candidate {
    index: usize,
    bound: PoolTimingChoice,
    rank: u64,
}

struct Winner {
    items: SelectedAreaItems,
    team: [u32; 5],
    order: [u32; 6],
    members: [MemberPower; 5],
    power: ScoringPower,
    skills: [ReplaySkill; 6],
    maximum: LayoutMaximum,
}

struct Search<'a> {
    cards: &'a [PreparedCard],
    replacement_groups: Vec<Vec<usize>>,
    catalogue: &'a SkillCatalogue,
    bound: &'a PoolTimingUpper,
    upper_cache: TimingUpperCache,
    score_cache: TimingCompileCache,
    upper_memo: Memo<TimingScoreUpper>,
    score_memo: Memo<CompiledTimingCandidates>,
    upper_scratch: TimingUpperScratch,
    score_scratch: TimingScoreScratch,
    work: u64,
    best: Option<Winner>,
}

impl Search<'_> {
    fn competitive(&self, upper: u64) -> bool {
        self.best
            .as_ref()
            .is_none_or(|best| upper > best.maximum.score)
    }

    fn relaxed_card(
        &self,
        index: usize,
        power: MemberPower,
    ) -> Result<PoolTimingChoice, DataError> {
        let cases = self.catalogue.cases[index];
        let power = power.linear_total();
        let mut choice = self.bound.choice(cases[0], power)?;
        for i in 1..cases.len() {
            if !cases[..i].contains(&cases[i]) {
                choice = choice.union(self.bound.choice(cases[i], power)?);
            }
        }
        Ok(choice)
    }

    fn item_upper(&self, item: &ItemCase) -> Result<u64, DataError> {
        let bound = item
            .powers
            .iter()
            .enumerate()
            .try_fold(PoolTimingChoice::default(), |union, (index, &power)| {
                self.relaxed_card(index, power).map(|row| union.union(row))
            })?;
        Ok(self.bound.separable_upper(&[bound; 5])?)
    }

    fn search_item(&mut self, item: &ItemCase) -> Result<(), DataError> {
        let mut candidates = Vec::with_capacity(self.cards.len());
        for group in &self.replacement_groups {
            for &i in group {
                let dominated = group.iter().any(|&j| {
                    i != j && item.powers[j].componentwise_dominates(item.powers[i])
                        && (j < i || !item.powers[i].componentwise_dominates(item.powers[j]))
                });
                if dominated {
                    continue;
                }
                let bound = self.relaxed_card(i, item.powers[i])?;
                let rank = self.bound.separable_upper(&[bound; 5])?;
                candidates.push(Candidate {
                    index: i,
                    bound,
                    rank,
                });
            }
        }
        candidates.sort_unstable_by_key(|row| (Reverse(row.rank), self.cards[row.index].card_id));
        let mut suffix = vec![PoolTimingChoice::default(); candidates.len() + 1];
        for i in (0..candidates.len()).rev() {
            suffix[i] = candidates[i].bound.union(suffix[i + 1]);
        }
        self.visit(
            item,
            &candidates,
            &suffix,
            &mut [0; 5],
            &mut [PoolTimingChoice::default(); 5],
            0,
            0,
        )
    }

    fn visit(
        &mut self,
        item: &ItemCase,
        cards: &[Candidate],
        suffix: &[PoolTimingChoice],
        roster: &mut [usize; 5],
        choices: &mut [PoolTimingChoice; 5],
        next: usize,
        depth: usize,
    ) -> Result<(), DataError> {
        self.work += 1;
        if self.work > MAX_SEARCH_NODES {
            return Err(engine_error(
                "击打搜索达到资源上限，未完成全部候选的最高分证明",
            ));
        }
        if depth == 5 {
            return self.score_roster(item, *roster);
        }
        let remaining = 5 - depth;
        if cards.len() - next < remaining {
            return Ok(());
        }
        // Remaining cards are selected at strictly increasing indices. Each
        // successive member therefore belongs to a smaller suffix.
        let relaxed = std::array::from_fn(|i| {
            if i < depth { choices[i] } else { suffix[next + (i - depth)] }
        });
        if !self.competitive(self.bound.separable_upper(&relaxed)?) {
            return Ok(());
        }
        for i in next..=cards.len() - remaining {
            let row = &cards[i];
            let character = self.cards[row.index].character_id;
            if roster[..depth]
                .iter()
                .any(|&j| self.cards[j].character_id == character)
            {
                continue;
            }
            roster[depth] = row.index;
            choices[depth] = row.bound;
            self.visit(item, cards, suffix, roster, choices, i + 1, depth + 1)?;
        }
        Ok(())
    }

    fn score_roster(&mut self, item: &ItemCase, roster: [usize; 5]) -> Result<(), DataError> {
        let cards = roster.map(|i| &self.cards[i]);
        let ids = self.catalogue.roster_ids(self.cards, roster);
        let members = roster.map(|i| item.powers[i]);
        let resolved = std::array::from_fn::<_, 5, _>(|i| {
            self.bound.choice(ids[i], members[i].linear_total())
        })
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
        if !self.competitive(self.bound.upper(&resolved.try_into().unwrap())?) {
            return Ok(());
        }

        let skills = ids.map(|i| self.catalogue.skills[i].clone());
        let axes = members.map(MemberPower::final_parameters);
        let layouts = TimingLayouts::for_skills(&skills, |layout| {
            ScoringPower::from_member_parameters(&layout.map(|i| axes[i]))
        })?;
        self.work += layouts.requests().len() as u64;
        if self.work > MAX_SEARCH_NODES {
            return Err(engine_error(
                "击打搜索达到资源上限，未完成全部候选的最高分证明",
            ));
        }
        let mut upper = vec![0; layouts.requests().len()];
        for group in layouts.configurations() {
            let sequence = layouts.requests()[group[0]]
                .slots
                .map(|i| skills[i].clone());
            let model = self
                .upper_memo
                .get(&sequence, || self.upper_cache.prepare(&sequence))?;
            for &i in group {
                upper[i] = model
                    .score(layouts.requests()[i].power, &mut self.upper_scratch)
                    .map_err(engine_error)?;
            }
        }

        let cutoff = self.best.as_ref().map(|best| best.maximum.score);
        if cutoff.is_some_and(|score| upper.iter().all(|&value| value <= score)) {
            return Ok(());
        }
        let maximum = layouts
            .maximize_optional(&upper, |i, request| {
                if cutoff.is_some_and(|score| upper[i] <= score) {
                    return Ok(None);
                }
                let sequence = request.slots.map(|i| skills[i].clone());
                let bank = self
                    .score_memo
                    .get(&sequence, || self.score_cache.prepare_for_score(&sequence))?;
                bank.score_optional(request.power, &mut self.score_scratch)
            })
            .map_err(engine_error)?;
        if let Some(maximum) = maximum.filter(|candidate| self.competitive(candidate.score)) {
            self.best = Some(Winner {
                items: item.items.clone(),
                team: maximum.recommended_layout.map(|i| cards[i].card_id),
                order: maximum.chosen_slots.map(|i| cards[i].card_id),
                members: maximum.recommended_layout.map(|i| members[i]),
                power: layouts.requests()[maximum.chosen_request].power,
                skills: maximum.chosen_slots.map(|i| skills[i].clone()),
                maximum,
            });
        }
        Ok(())
    }
}

pub(super) fn maximize(
    input: TimingMaximizeInput<'_>,
    restrictions: Option<TimingRangeRequest>,
    live: LiveVariant,
) -> Result<BuildResult, DataError> {
    let start = Instant::now();
    let selections = timing_selections(&input, 1, restrictions.as_ref())?;
    let song = &selections[0];
    let raw = &input.charts[0];
    let chart = TimeDistanceChart::from_bestdori(&raw.data)?;
    let mut replay_input = chart.input(TimeDistanceScoring {
        power: 0.0,
        play_level: raw.play_level,
        skills: vec![
            ReplaySkill {
                duration: 1.0,
                score: ScoreEffect::Plain { percent: 0.0 },
                correction: None
            };
            6
        ],
        initial_combo: 0,
        medley_combo: false,
        fever: false,
    })?;
    let limits = restrictions
        .as_ref()
        .map(|r| r.songs[0].limits(&replay_input))
        .transpose()?
        .unwrap_or_else(|| vec![None; replay_input.notes.len()]);
    let inventory = prepare_inventory(&input, 5)?;
    let cards = &inventory.prepared;
    let character_count = cards
        .iter()
        .map(|card| card.character_id)
        .collect::<BTreeSet<_>>()
        .len();
    if character_count < 5 {
        return Err(MaximizeError::NotEnoughDistinctCharacters {
            count: character_count,
        }
        .into());
    }
    let catalogue = SkillCatalogue::new(&inventory.data, input.server, cards)?;
    let area =
        calculate_area_item_percent(&inventory.player.area_item, input.area_item_definitions)?;
    let items = area_item_combinations(&area)
        .into_iter()
        .map(|items| {
            let equipment = selected_equipment(
                &inventory.player.area_item,
                input.area_item_definitions,
                &items,
                &inventory.item_order,
            )
            .map_err(engine_error)?;
            let powers = inventory
                .native
                .iter()
                .map(|card| card.power(&equipment))
                .collect::<Result<Vec<_>, _>>()
                .map_err(engine_error)?;
            Ok::<_, DataError>(ItemCase { items, powers })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if items.is_empty() {
        return Err(MaximizeError::NoAreaItemCombinations.into());
    }
    // Use the same outward native-f32 accumulation envelope as the Medley pool.
    let max_member = items
        .iter()
        .flat_map(|case| &case.powers)
        .map(|power| power.linear_total())
        .fold(0.0, f64::max);
    let cap = ScoringPower::new((max_member * 5.005 + 1.0) as f32).map_err(engine_error)?;
    let bound = PoolTimingUpper::new_adjacent_net(
        &replay_input,
        &catalogue.skills,
        cap,
        Default::default(),
    )?;
    let mut replacements = BTreeMap::new();
    for (i, card) in cards.iter().enumerate() {
        let key = (card.character_id, card.band_id, card.attribute.as_str(), catalogue.cases[i]);
        replacements.entry(key).or_insert_with(Vec::new).push(i);
    }
    let mut search = Search {
        cards,
        replacement_groups: replacements.into_values().collect(),
        catalogue: &catalogue,
        bound: &bound,
        upper_cache: TimingUpperCache::new(&replay_input),
        score_cache: TimingCompileCache::new(&replay_input, Default::default()),
        upper_memo: Memo::new(64),
        score_memo: Memo::new(32),
        upper_scratch: TimingUpperScratch::default(),
        score_scratch: TimingScoreScratch::default(),
        work: 0,
        best: None,
    };
    if limits.iter().any(Option::is_some) {
        search.upper_cache = search.upper_cache.with_frame_ranges(limits.clone());
        search.score_cache = search.score_cache.with_frame_ranges(limits.clone());
    }
    let mut active = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let dominated = items
            .iter()
            .enumerate()
            .any(|(j, other)| i != j && other.dominates(item) && (j < i || !item.dominates(other)));
        if !dominated {
            active.push((search.item_upper(item)?, i));
        }
    }
    active.sort_unstable_by_key(|&(upper, index)| (Reverse(upper), index));
    for &(upper, index) in &active {
        if !search.competitive(upper) {
            break;
        }
        search.search_item(&items[index])?;
    }
    let winner = search
        .best
        .take()
        .ok_or_else(|| engine_error("当前击打范围内没有可行方案"))?;
    let deck = DeckPower::from_members(&winner.members).map_err(engine_error)?;
    if deck.scoring() != winner.power {
        return Err(engine_error("最终摆位的计分综合力与求解结果不一致"));
    }
    let bank = search
        .score_cache
        .prepare(&winner.skills)
        .map_err(engine_error)?;
    let result = bank.materialize(winner.power).map_err(engine_error)?;
    replay_input.skills = winner.skills.to_vec();
    replay_input.power = winner.power.value();
    let result = restore_chart_positions(&replay_input, &limits, result);
    replay_input.hit_frames = result.hit_frames.clone();
    replay_input.hit_order = result.hit_order.clone();
    if result.total != winner.maximum.score || replay(&replay_input)? != result {
        return Err(engine_error("最终击打记录无法按最高分精确回放"));
    }
    let score = int_score(result.total).map_err(engine_error)?;
    let stat = deck.display_for_live(live).map_err(engine_error)?;
    let team_order = timing_team_order(winner.team, winner.order, &winner.maximum)?;
    Ok(BuildResult {
        scoring_revision: Some(ARITHMETIC_REVISION.to_owned()),
        skill_queue_notices: Vec::new(),
        event_id: input.event_id,
        event_type: input.event.event_type,
        total_score: score,
        total_stat: stat,
        songs: vec![SongBuildResult {
            song_id: song.song_id,
            difficulty: song.difficulty,
            score,
            stat,
            team_card_ids: winner.team.to_vec(),
            captain_card_id: winner.team[2],
            team_order: Some(team_order),
            skill_queue_risk: false,
        }],
        items: Some(winner.items),
        solver: Some("timingExact".to_owned()),
        metrics: Some(CalculationMetrics {
            card_count: cards.len(),
            song_count: 1,
            item_combinations_before: items.len(),
            item_combinations_after: active.len(),
            total_elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
            ..Default::default()
        }),
        hit_timing: Some(timing_result(
            vec![HitTimingSongResult {
                song_id: song.song_id,
                difficulty: song.difficulty,
                input: replay_input,
                result,
            }],
            cards.len(),
            items.len(),
            restrictions,
        )),
    })
}
