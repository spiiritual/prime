use super::*;

use super::image_assets::{LoadoutMetadata, cache_loadout_images, fetch_loadout_metadata};
use super::session::{ApiIdentity, resolve_credentials};
use super::shop::{
    RADIANITE_POINTS_UUID, format_whole_number, remaining_seconds_at, shop_currency_name,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct LoadoutResult {
    pub(in crate::ui) account_id: AccountId,
    pub(in crate::ui) summary: LoadoutSummary,
    pub(in crate::ui) session: AuthSession,
    pub(in crate::ui) launcher_session: Option<LauncherSessionBackup>,
    pub(in crate::ui) identity: ApiIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct LoadoutSummary {
    /// `None` when Riot reported no usable level (0 or hidden), so a known level is kept.
    pub(in crate::ui) account_level: Option<i64>,
    /// The equipped player card's ID, when the loadout loaded.
    pub(in crate::ui) player_card_id: Option<String>,
    pub(in crate::ui) gun_skins: Vec<LoadoutGunDisplay>,
    pub(in crate::ui) loadout_error: Option<String>,
    pub(in crate::ui) battle_pass: Option<BattlePassProgressDisplay>,
    pub(in crate::ui) battle_pass_error: Option<String>,
}

impl LoadoutSummary {
    pub(in crate::ui) fn from_response(
        response: PlayerLoadoutResponse,
        skins: &SkinCatalog,
        weapons: &WeaponCatalog,
        account_level: Option<i64>,
    ) -> Self {
        let mut gun_skins = response
            .guns
            .into_iter()
            .map(|gun| {
                let weapon = weapons.resolve(&gun.id);
                let default_skin = weapon
                    .default_skin_uuid
                    .as_deref()
                    .is_some_and(|id| id.eq_ignore_ascii_case(&gun.skin_id));
                let weapon = WeaponDisplay::from(weapon);
                let base_skin = skins.resolve(&gun.skin_id);
                let skin_level = skins.resolve(&gun.skin_level_id).level_label;
                let chroma = skins.resolve(&gun.chroma_id).chroma_label;
                let skin = SkinDisplay::from(resolve_current_skin(
                    skins,
                    &gun.skin_id,
                    &gun.skin_level_id,
                    &gun.chroma_id,
                ));

                LoadoutGunDisplay {
                    weapon,
                    skin,
                    skin_name: base_skin.display_name,
                    skin_level,
                    chroma,
                    default_skin,
                }
            })
            .collect::<Vec<_>>();
        gun_skins.sort_by_key(|gun| weapon_order(&gun.weapon.display_name));

        Self {
            account_level: known_account_level(account_level)
                .or_else(|| known_account_level(Some(response.identity.account_level))),
            player_card_id: Some(response.identity.player_card_id)
                .filter(|id| !id.trim().is_empty()),
            gun_skins,
            loadout_error: None,
            battle_pass: None,
            battle_pass_error: None,
        }
    }

    /// A summary for when the equipped loadout could not be loaded, so battle pass progress can
    /// still be shown.
    pub(in crate::ui) fn without_loadout(error: String, account_level: Option<i64>) -> Self {
        Self {
            account_level: known_account_level(account_level),
            player_card_id: None,
            gun_skins: Vec::new(),
            loadout_error: Some(error),
            battle_pass: None,
            battle_pass_error: None,
        }
    }

    /// A pass that had already ended when it loaded has nothing to count down.
    pub(in crate::ui) fn battle_pass_timer_active(&self) -> bool {
        self.battle_pass
            .as_ref()
            .and_then(|battle_pass| battle_pass.remaining_seconds)
            .is_some_and(|seconds| seconds > 0)
    }

    /// Whether the battle pass shown has ended since it loaded, so the next act can be loaded.
    pub(in crate::ui) fn battle_pass_ended_at(&self, now: iced::time::Instant) -> bool {
        self.battle_pass_timer_active()
            && self
                .battle_pass
                .as_ref()
                .and_then(|battle_pass| battle_pass.remaining_seconds_at(now))
                .is_some_and(|seconds| seconds <= 0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct BattlePassProgressDisplay {
    pub(in crate::ui) name: String,
    pub(in crate::ui) season_name: Option<String>,
    pub(in crate::ui) level_reached: i64,
    pub(in crate::ui) total_levels: Option<i64>,
    pub(in crate::ui) epilogue_levels: i64,
    pub(in crate::ui) progression_towards_next_level: i64,
    pub(in crate::ui) next_level_progress_required: Option<i64>,
    pub(in crate::ui) total_progression_earned: i64,
    pub(in crate::ui) total_progression_required: Option<i64>,
    pub(in crate::ui) completed: bool,
    pub(in crate::ui) remaining_seconds: Option<i64>,
    pub(in crate::ui) chapters: Vec<BattlePassChapterDisplay>,
    /// The chapter the page shows: the one with the next tier, until another is picked.
    pub(in crate::ui) selected_chapter: usize,
    pub(in crate::ui) paid_pass_owned: bool,
    pub(in crate::ui) loaded_at: iced::time::Instant,
}

/// The tiers the game groups together, usually 5, with the free rewards at the chapter's end.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct BattlePassChapterDisplay {
    /// Counted apart for the main pass and the epilogue: Chapter 1..11, then Epilogue 1..2.
    pub(in crate::ui) number: i64,
    pub(in crate::ui) is_epilogue: bool,
    pub(in crate::ui) first_tier: i64,
    pub(in crate::ui) last_tier: i64,
    /// Each tier's premium reward in order, then the free rewards.
    pub(in crate::ui) rewards: Vec<BattlePassRewardDisplay>,
    /// Its art is downloading, having been opened after the page loaded.
    pub(in crate::ui) art_loading: bool,
}

impl BattlePassChapterDisplay {
    pub(in crate::ui) fn name(&self) -> String {
        let kind = if self.is_epilogue {
            "Epilogue"
        } else {
            "Chapter"
        };
        format!("{kind} {}", self.number)
    }

    pub(in crate::ui) fn short_name(&self) -> String {
        let kind = if self.is_epilogue { "EP" } else { "CH" };
        format!("{kind} {}", self.number)
    }

    fn holds(&self, tier: i64) -> bool {
        (self.first_tier..=self.last_tier).contains(&tier)
    }
}

impl BattlePassProgressDisplay {
    /// The rewards whose art is downloaded with the page: the open chapter's and the weapon skins
    /// ahead that show. Other chapters' art loads when they're opened.
    pub(in crate::ui) fn shown_rewards_mut(
        &mut self,
    ) -> impl Iterator<Item = &mut BattlePassRewardDisplay> {
        let selected = self.selected_chapter;
        let skins: Vec<String> = self.skins_ahead().map(|skin| skin.uuid.clone()).collect();
        self.chapters
            .iter_mut()
            .enumerate()
            .flat_map(move |(index, chapter)| {
                let skins = skins.clone();
                chapter
                    .rewards
                    .iter_mut()
                    .filter(move |reward| index == selected || skins.contains(&reward.uuid))
            })
    }

    pub(in crate::ui) fn selected_chapter(&self) -> Option<&BattlePassChapterDisplay> {
        self.chapters.get(self.selected_chapter)
    }

    /// The next few weapon skins in tiers not reached yet, in tier order.
    pub(in crate::ui) fn skins_ahead(&self) -> impl Iterator<Item = &BattlePassRewardDisplay> {
        self.chapters
            .iter()
            .flat_map(|chapter| &chapter.rewards)
            .filter(|reward| reward.is_skin_ahead_of(self.level_reached))
            .take(SKINS_AHEAD)
    }

    /// The tier being worked towards, while the pass isn't complete.
    pub(in crate::ui) fn next_tier(&self) -> Option<i64> {
        (!self.completed).then_some(self.level_reached.max(0) + 1)
    }

    fn current_chapter(&self) -> usize {
        self.next_tier()
            .and_then(|tier| self.chapters.iter().position(|chapter| chapter.holds(tier)))
            .unwrap_or(self.chapters.len().saturating_sub(1))
    }

    /// A free reward once its tier is reached; a premium one only with the premium pass too.
    pub(in crate::ui) fn is_earned(&self, reward: &BattlePassRewardDisplay) -> bool {
        reward.tier <= self.level_reached && !self.is_locked(reward)
    }

    /// Premium rewards can't be earned without the premium pass, whatever the tier.
    pub(in crate::ui) fn is_locked(&self, reward: &BattlePassRewardDisplay) -> bool {
        !reward.free && !self.paid_pass_owned
    }

    pub(in crate::ui) fn title(&self) -> String {
        self.season_name
            .as_ref()
            .filter(|name| !name.trim().is_empty())
            .map(|season| format!("{season} Battle Pass"))
            .unwrap_or_else(|| self.name.clone())
    }

    /// What the big number counts ("TIER", or "EPILOGUE" past the main pass), the number, and
    /// what it's out of, when known.
    pub(in crate::ui) fn tier_display(&self) -> (&'static str, i64, Option<i64>) {
        match self.total_levels {
            Some(total) if total > 0 => {
                let epilogue_reached = self.level_reached - total;
                if self.epilogue_levels > 0 && epilogue_reached > 0 {
                    (
                        "EPILOGUE",
                        epilogue_reached.min(self.epilogue_levels),
                        Some(self.epilogue_levels),
                    )
                } else {
                    ("TIER", self.level_reached.clamp(0, total), Some(total))
                }
            }
            _ => ("TIER", self.level_reached.max(0), None),
        }
    }

    /// "69% completed · 152,400 XP left" for the main pass, or just "Completed".
    pub(in crate::ui) fn completion_label(&self) -> Option<String> {
        if self.completed {
            return Some("Completed".to_string());
        }
        let required = self
            .total_progression_required
            .filter(|required| *required > 0)?;
        let left = (required - self.total_progression_earned.max(0)).max(0);
        let percent = (self.progress_fraction() * 100.0).floor();
        Some(if left > 0 {
            format!(
                "{percent}% completed · {} XP left",
                format_whole_number(left)
            )
        } else {
            format!("{percent}% completed")
        })
    }

    pub(in crate::ui) fn next_tier_label(&self) -> String {
        if self.completed {
            return "Complete".to_string();
        }

        match self.next_level_progress_required {
            Some(required) if required > 0 => format!(
                "{} / {} XP",
                format_whole_number(self.progression_towards_next_level.max(0)),
                format_whole_number(required)
            ),
            _ => format!(
                "{} XP",
                format_whole_number(self.progression_towards_next_level.max(0))
            ),
        }
    }

    /// How far into the next tier, from 0 to 1.
    pub(in crate::ui) fn next_tier_fraction(&self) -> f32 {
        match self.next_level_progress_required {
            Some(required) if required > 0 && !self.completed => {
                (self.progression_towards_next_level.max(0) as f32 / required as f32)
                    .clamp(0.0, 1.0)
            }
            _ => 0.0,
        }
    }

    /// "Tier 36", or "Epilogue 2" for the epilogue's second tier, with the amount when more than
    /// one: "Tier 12 · x2".
    pub(in crate::ui) fn reward_tier_label(&self, reward: &BattlePassRewardDisplay) -> String {
        let mut label = self.reward_tier(reward);
        if let Some(amount) = reward.amount_label() {
            label = format!("{label} · {amount}");
        }
        label
    }

    /// "Tier 36", or "Epilogue 2" for the epilogue's second tier.
    pub(in crate::ui) fn reward_tier(&self, reward: &BattlePassRewardDisplay) -> String {
        if reward.is_epilogue {
            format!("Epilogue {}", self.epilogue_tier(reward.tier))
        } else {
            format!("Tier {}", reward.tier.max(0))
        }
    }

    /// "Tiers 36–40", or "Epilogue 1–5".
    pub(in crate::ui) fn chapter_tiers_label(&self, chapter: &BattlePassChapterDisplay) -> String {
        if chapter.is_epilogue {
            format!(
                "Epilogue {}–{}",
                self.epilogue_tier(chapter.first_tier),
                self.epilogue_tier(chapter.last_tier)
            )
        } else {
            format!("Tiers {}–{}", chapter.first_tier, chapter.last_tier)
        }
    }

    /// Tiers count on through the epilogue, which numbers its own from 1.
    fn epilogue_tier(&self, tier: i64) -> i64 {
        let main_tiers = self
            .chapters
            .iter()
            .filter(|chapter| !chapter.is_epilogue)
            .map(|chapter| chapter.last_tier)
            .max()
            .unwrap_or(0);
        (tier - main_tiers).max(0)
    }

    pub(in crate::ui) fn pass_label(&self) -> &'static str {
        if self.paid_pass_owned {
            "Premium"
        } else {
            "Free"
        }
    }

    pub(in crate::ui) fn progress_fraction(&self) -> f32 {
        if self.completed {
            return 1.0;
        }

        self.total_progression_required
            .filter(|required| *required > 0)
            .map(|required| {
                (self.total_progression_earned.max(0) as f32 / required as f32).clamp(0.0, 1.0)
            })
            .unwrap_or(0.0)
    }

    pub(in crate::ui) fn remaining_seconds_at(&self, now: iced::time::Instant) -> Option<i64> {
        self.remaining_seconds
            .map(|seconds| remaining_seconds_at(seconds, self.loaded_at, now))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct BattlePassRewardDisplay {
    pub(in crate::ui) tier: i64,
    pub(in crate::ui) is_epilogue: bool,
    /// On the free track, which every account earns; the rest need the premium pass.
    pub(in crate::ui) free: bool,
    pub(in crate::ui) uuid: String,
    pub(in crate::ui) name: String,
    pub(in crate::ui) kind: String,
    pub(in crate::ui) amount: i64,
    pub(in crate::ui) display_icon: Option<String>,
    pub(in crate::ui) viewer_icon: Option<String>,
    pub(in crate::ui) cached_icon: Option<PathBuf>,
}

impl BattlePassRewardDisplay {
    pub(in crate::ui) fn amount_label(&self) -> Option<String> {
        (self.kind != "Currency" && self.amount > 1)
            .then(|| format!("x{}", format_whole_number(self.amount)))
    }

    fn is_skin_ahead_of(&self, level_reached: i64) -> bool {
        self.kind == WEAPON_SKIN && self.tier > level_reached
    }
}

const WEAPON_SKIN: &str = "Weapon skin";
/// How many of the weapon skins ahead the page shows.
pub(in crate::ui) const SKINS_AHEAD: usize = 3;

pub(in crate::ui) fn battle_pass_progress_from_responses(
    contracts: &ContractsResponse,
    contract_catalog: &ContractCatalog,
    content: Option<&GameContentResponse>,
    skins: &SkinCatalog,
    accessories: &AccessoryCatalog,
    currencies: &CurrencyCatalog,
) -> Option<BattlePassProgressDisplay> {
    battle_pass_progress_from_responses_at(BattlePassProgressContext {
        contracts,
        contract_catalog,
        content,
        skins,
        accessories,
        currencies,
        now_utc: OffsetDateTime::now_utc(),
        loaded_at: iced::time::Instant::now(),
    })
}

struct BattlePassProgressContext<'a> {
    contracts: &'a ContractsResponse,
    contract_catalog: &'a ContractCatalog,
    content: Option<&'a GameContentResponse>,
    skins: &'a SkinCatalog,
    accessories: &'a AccessoryCatalog,
    currencies: &'a CurrencyCatalog,
    now_utc: OffsetDateTime,
    loaded_at: iced::time::Instant,
}

fn battle_pass_progress_from_responses_at(
    context: BattlePassProgressContext<'_>,
) -> Option<BattlePassProgressDisplay> {
    let active_act = context.content.and_then(GameContentResponse::active_act);
    let (definition, contract) =
        find_battle_pass_contract(context.contracts, context.contract_catalog, active_act)?;
    // The act's name and end time only describe the contract when it is that act's battle pass;
    // an older season's fallback contract keeps its own name and has no countdown.
    let contract_act =
        active_act.filter(|act| definition.relation_uuid.eq_ignore_ascii_case(&act.id));
    let progression_deltas = definition.level_xp.as_slice();
    // Epilogue tiers come after the main pass and aren't part of its total.
    let is_epilogue = |index: usize| {
        definition
            .reward_levels
            .get(index)
            .is_some_and(|level| level.is_epilogue)
    };
    let main_levels = (0..progression_deltas.len())
        .filter(|index| !is_epilogue(*index))
        .count();
    let total_levels = Some(i64::try_from(main_levels).unwrap_or(0));
    let epilogue_levels = i64::try_from(progression_deltas.len() - main_levels).unwrap_or(0);
    let total_progression_required = Some(
        progression_deltas
            .iter()
            .enumerate()
            .filter(|(index, _)| !is_epilogue(*index))
            .map(|(_, xp)| *xp)
            .sum::<i64>(),
    );
    let next_level_index = usize::try_from(contract.progression_level_reached.max(0)).ok();
    let next_level_progress_required =
        next_level_index.and_then(|index| progression_deltas.get(index).copied());
    let all_levels = i64::try_from(progression_deltas.len()).unwrap_or(0);
    let completed = contract.progression_completed
        || (all_levels > 0 && contract.progression_level_reached >= all_levels);
    let remaining_seconds =
        contract_act.and_then(|act| remaining_seconds_until_utc_at(&act.end_time, context.now_utc));
    let paid_pass_owned = battle_pass_paid_pass_owned(definition, contract);
    let chapters = battle_pass_chapters(
        definition,
        context.skins,
        context.accessories,
        context.currencies,
    );

    let mut progress = BattlePassProgressDisplay {
        name: non_empty_string(definition.display_name.clone())
            .unwrap_or_else(|| "Battle Pass".to_string()),
        season_name: contract_act.and_then(|act| non_empty_string(act.name.clone())),
        level_reached: contract.progression_level_reached,
        total_levels,
        epilogue_levels,
        progression_towards_next_level: contract.progression_towards_next_level,
        next_level_progress_required,
        total_progression_earned: contract.contract_progression.total_progression_earned,
        total_progression_required,
        completed,
        remaining_seconds,
        chapters,
        selected_chapter: 0,
        paid_pass_owned,
        loaded_at: context.loaded_at,
    };
    progress.selected_chapter = progress.current_chapter();
    Some(progress)
}

fn battle_pass_paid_pass_owned(definition: &ResolvedContract, contract: &PlayerContract) -> bool {
    let Some(schedule_id) = definition
        .premium_reward_schedule_uuid
        .as_ref()
        .filter(|schedule_id| !schedule_id.trim().is_empty())
    else {
        return true;
    };

    contract
        .contract_progression
        .highest_rewarded_level
        .iter()
        .any(|(reward_schedule_id, level)| {
            reward_schedule_id.eq_ignore_ascii_case(schedule_id) && level.amount > 0
        })
}

fn battle_pass_chapters(
    definition: &ResolvedContract,
    skins: &SkinCatalog,
    accessories: &AccessoryCatalog,
    currencies: &CurrencyCatalog,
) -> Vec<BattlePassChapterDisplay> {
    let mut chapters: Vec<BattlePassChapterDisplay> = Vec::new();
    let mut chapter_index = None;

    for level in &definition.reward_levels {
        if chapter_index != Some(level.chapter) {
            chapter_index = Some(level.chapter);
            let number = chapters
                .iter()
                .filter(|chapter| chapter.is_epilogue == level.is_epilogue)
                .count();
            chapters.push(BattlePassChapterDisplay {
                number: i64::try_from(number + 1).unwrap_or(i64::MAX),
                is_epilogue: level.is_epilogue,
                first_tier: level.tier,
                last_tier: level.tier,
                rewards: Vec::new(),
                art_loading: false,
            });
        }
        let Some(chapter) = chapters.last_mut() else {
            continue;
        };
        chapter.last_tier = level.tier;

        let display = |reward, free| {
            battle_pass_reward_display(reward, level, free, skins, accessories, currencies)
        };
        // Free rewards sit on a chapter's last tier, so they still come after its premium ones.
        chapter.rewards.extend(
            level
                .premium_reward
                .iter()
                .map(|reward| display(reward, false)),
        );
        chapter.rewards.extend(
            level
                .free_rewards
                .iter()
                .map(|reward| display(reward, true)),
        );
    }

    chapters
}

fn battle_pass_reward_display(
    reward: &ResolvedContractReward,
    level: &crate::riot::content::ResolvedContractRewardLevel,
    free: bool,
    skins: &SkinCatalog,
    accessories: &AccessoryCatalog,
    currencies: &CurrencyCatalog,
) -> BattlePassRewardDisplay {
    let resolved = resolve_battle_pass_reward(reward, skins, accessories, currencies);

    BattlePassRewardDisplay {
        tier: level.tier,
        is_epilogue: level.is_epilogue,
        free,
        uuid: reward.uuid.clone(),
        name: resolved.name,
        kind: resolved.kind,
        amount: reward.amount,
        display_icon: resolved.display_icon,
        viewer_icon: resolved.viewer_icon,
        cached_icon: None,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ResolvedBattlePassReward {
    name: String,
    kind: String,
    display_icon: Option<String>,
    viewer_icon: Option<String>,
}

fn resolve_battle_pass_reward(
    reward: &ResolvedContractReward,
    skins: &SkinCatalog,
    accessories: &AccessoryCatalog,
    currencies: &CurrencyCatalog,
) -> ResolvedBattlePassReward {
    match reward.kind.as_str() {
        "EquippableSkinLevel" | "EquippableSkinChroma" | "Skin" => {
            let skin = skins.resolve(&reward.uuid);
            ResolvedBattlePassReward {
                name: skin.display_name,
                kind: WEAPON_SKIN.to_string(),
                display_icon: skin.display_icon,
                viewer_icon: skin.viewer_icon,
            }
        }
        "EquippableCharmLevel" | "Buddy" | "BuddyLevel" => {
            let accessory = accessories.resolve(&reward.uuid);
            ResolvedBattlePassReward {
                name: accessory.display_name,
                kind: "Gun buddy".to_string(),
                display_icon: accessory.display_icon,
                viewer_icon: accessory.viewer_icon,
            }
        }
        "Spray" => {
            let accessory = accessories.resolve(&reward.uuid);
            ResolvedBattlePassReward {
                name: accessory.display_name,
                kind: "Spray".to_string(),
                display_icon: accessory.display_icon,
                viewer_icon: accessory.viewer_icon,
            }
        }
        "PlayerCard" => {
            let accessory = accessories.resolve(&reward.uuid);
            ResolvedBattlePassReward {
                name: accessory.display_name,
                kind: "Player card".to_string(),
                // The card's tall art, which suits a reward card better than its square icon.
                display_icon: accessory.viewer_icon.clone(),
                viewer_icon: accessory.viewer_icon,
            }
        }
        "Totem" | "Flex" => {
            let accessory = accessories.resolve(&reward.uuid);
            ResolvedBattlePassReward {
                name: accessory.display_name,
                kind: "Flex".to_string(),
                display_icon: accessory.display_icon,
                viewer_icon: accessory.viewer_icon,
            }
        }
        "Title" => {
            let accessory = accessories.resolve(&reward.uuid);
            ResolvedBattlePassReward {
                name: accessory.display_name,
                kind: "Title".to_string(),
                display_icon: accessory.display_icon,
                viewer_icon: accessory.viewer_icon,
            }
        }
        "Currency" => {
            let currency = currencies.resolve(&reward.uuid);
            let name = shop_currency_name(&currency.display_name);
            let amount = battle_pass_currency_reward_amount(reward, &currency);
            ResolvedBattlePassReward {
                name: format!("{} {name}", format_whole_number(amount)),
                kind: "Currency".to_string(),
                display_icon: currency.display_icon,
                viewer_icon: currency.viewer_icon,
            }
        }
        kind => ResolvedBattlePassReward {
            name: reward.uuid.clone(),
            kind: if kind.trim().is_empty() {
                "Reward".to_string()
            } else {
                kind.to_string()
            },
            display_icon: None,
            viewer_icon: None,
        },
    }
}

fn battle_pass_currency_reward_amount(
    reward: &ResolvedContractReward,
    currency: &ResolvedCurrency,
) -> i64 {
    if reward.amount == 1 && currency.uuid.eq_ignore_ascii_case(RADIANITE_POINTS_UUID) {
        10
    } else {
        reward.amount
    }
}

fn find_battle_pass_contract<'a>(
    contracts: &'a ContractsResponse,
    contract_catalog: &'a ContractCatalog,
    active_act: Option<&GameContentSeason>,
) -> Option<(&'a ResolvedContract, &'a PlayerContract)> {
    if let Some((definition, contract)) = active_act
        .and_then(|act| contract_catalog.resolve_active_season(&act.id))
        .filter(|definition| !definition.level_xp.is_empty())
        .and_then(|definition| {
            contracts
                .contracts
                .iter()
                .find(|contract| {
                    contract
                        .contract_definition_id
                        .eq_ignore_ascii_case(&definition.uuid)
                })
                .map(|contract| (definition, contract))
        })
    {
        return Some((definition, contract));
    }

    contracts
        .contracts
        .iter()
        .filter_map(|contract| {
            contract_catalog
                .resolve(&contract.contract_definition_id)
                .filter(|definition| {
                    definition.is_season_contract() && !definition.level_xp.is_empty()
                })
                .map(|definition| (definition, contract))
        })
        .max_by_key(|(_, contract)| {
            (
                !contract.progression_completed,
                contract.progression_level_reached,
                contract.contract_progression.total_progression_earned,
            )
        })
}

fn remaining_seconds_until_utc_at(end_time: &str, now: OffsetDateTime) -> Option<i64> {
    let end = OffsetDateTime::parse(end_time.trim(), &Rfc3339).ok()?;

    Some(
        end.unix_timestamp()
            .saturating_sub(now.unix_timestamp())
            .max(0),
    )
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct LoadoutGunDisplay {
    pub(in crate::ui) weapon: WeaponDisplay,
    pub(in crate::ui) skin: SkinDisplay,
    pub(in crate::ui) skin_name: String,
    pub(in crate::ui) skin_level: Option<String>,
    /// The equipped variant, when it isn't the skin's base look.
    pub(in crate::ui) chroma: Option<String>,
    /// Whether this is the weapon's standard skin rather than one the account got.
    pub(in crate::ui) default_skin: bool,
}

impl LoadoutGunDisplay {
    pub(in crate::ui) fn skin_detail_label(&self) -> String {
        [self.skin_level.as_deref(), self.chroma.as_deref()]
            .into_iter()
            .flatten()
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .fold(self.skin_name.clone(), |label, part| {
                format!("{label} - {part}")
            })
    }

    #[cfg(test)]
    pub(in crate::ui) fn label(&self) -> String {
        format!("{}: {}", self.weapon.display_name, self.skin_detail_label())
    }
}

pub(in crate::ui) fn weapon_order(name: &str) -> (usize, String) {
    const WEAPON_ORDER: &[&str] = &[
        "Classic", "Shorty", "Frenzy", "Ghost", "Sheriff", "Bandit", "Stinger", "Spectre", "Bucky",
        "Judge", "Bulldog", "Guardian", "Phantom", "Vandal", "Marshal", "Outlaw", "Operator",
        "Ares", "Odin", "Melee",
    ];
    let index = WEAPON_ORDER
        .iter()
        .position(|weapon| *weapon == name)
        .unwrap_or(99);

    (index, name.to_string())
}

/// Maps Riot's equippable category to a collection section, so new weapons land in the right
/// section without a code change.
pub(in crate::ui) fn weapon_category(category: Option<&str>) -> &'static str {
    let category = category.unwrap_or_default();
    let category = category
        .strip_prefix("EEquippableCategory::")
        .unwrap_or(category);

    match category {
        "Sidearm" => "Sidearms",
        "SMG" => "SMGs",
        "Shotgun" => "Shotguns",
        "Rifle" => "Rifles",
        "Sniper" => "Sniper Rifles",
        "Heavy" => "Heavy",
        "Melee" => "Melee",
        _ => "Other",
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct WeaponDisplay {
    pub(in crate::ui) uuid: String,
    pub(in crate::ui) display_name: String,
    pub(in crate::ui) category: &'static str,
    pub(in crate::ui) display_icon: Option<String>,
    pub(in crate::ui) viewer_icon: Option<String>,
}

impl From<ResolvedWeapon> for WeaponDisplay {
    fn from(weapon: ResolvedWeapon) -> Self {
        Self {
            uuid: weapon.uuid,
            display_name: weapon.display_name,
            category: weapon_category(weapon.category.as_deref()),
            display_icon: weapon.display_icon,
            viewer_icon: weapon.viewer_icon,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct SkinDisplay {
    pub(in crate::ui) uuid: String,
    pub(in crate::ui) display_name: String,
    pub(in crate::ui) display_icon: Option<String>,
    pub(in crate::ui) viewer_icon: Option<String>,
    pub(in crate::ui) rarity: Option<String>,
    pub(in crate::ui) weapon_name: Option<String>,
    pub(in crate::ui) cached_icon: Option<PathBuf>,
}

impl From<ResolvedSkin> for SkinDisplay {
    fn from(skin: ResolvedSkin) -> Self {
        Self {
            uuid: skin.uuid,
            display_name: skin.display_name,
            display_icon: skin.display_icon,
            viewer_icon: skin.viewer_icon,
            rarity: skin.rarity,
            weapon_name: skin.weapon_name,
            cached_icon: None,
        }
    }
}

pub(in crate::ui) async fn fetch_loadout(
    account: AccountProfile,
    client_version: String,
    image_cache: ImageCache,
) -> Result<LoadoutResult, String> {
    let api = RiotApi::shared().map_err(|error| error.to_string())?;
    // Signing in doesn't need the catalogs, so both run at once.
    let (resolved, metadata) = iced::futures::join!(
        resolve_credentials(&api, &account, client_version),
        fetch_loadout_metadata()
    );
    let resolved = resolved?;
    let (account_xp, battle_pass, loadout) = iced::futures::join!(
        api.account_xp(&resolved.credentials),
        async {
            match &metadata.battle_pass {
                Ok(catalogs) => {
                    fetch_battle_pass_progress(&api, &resolved.credentials, catalogs).await
                }
                Err(error) => Err(error.clone()),
            }
        },
        async {
            match &metadata.weapon_content {
                Ok(_) => api
                    .player_loadout(&resolved.credentials)
                    .await
                    .map_err(|error| error.to_string()),
                Err(error) => Err(error.clone()),
            }
        },
    );
    let account_level = account_xp.ok().map(|xp| xp.progress.level);
    let loadout = loadout.and_then(|response| {
        let weapon_content = metadata.weapon_content.as_ref().map_err(Clone::clone)?;
        Ok(LoadoutSummary::from_response(
            response,
            &weapon_content.skins,
            &weapon_content.weapons,
            account_level,
        ))
    });

    let mut summary = combine_loadout_sections(loadout, battle_pass, account_level)?;
    cache_loadout_images(&mut summary, &image_cache).await;

    Ok(LoadoutResult {
        account_id: account.id,
        summary,
        session: resolved.session,
        launcher_session: resolved.launcher_session,
        identity: resolved.identity,
    })
}

/// The loadout and battle pass are separate sub-tabs, so one failing doesn't hide the other.
/// Only when both fail is the load an error, and it names both failures. A battle pass of
/// `Ok(None)` means the account has no progress this act, which isn't a failure.
pub(in crate::ui) fn combine_loadout_sections(
    loadout: Result<LoadoutSummary, String>,
    battle_pass: Result<Option<BattlePassProgressDisplay>, String>,
    account_level: Option<i64>,
) -> Result<LoadoutSummary, String> {
    let mut summary = match (loadout, &battle_pass) {
        (Ok(summary), _) => summary,
        (Err(error), Ok(_)) => LoadoutSummary::without_loadout(error, account_level),
        (Err(loadout_error), Err(battle_pass_error)) => {
            return Err(format!("{loadout_error}; battle pass: {battle_pass_error}"));
        }
    };
    match battle_pass {
        Ok(progress) => {
            summary.battle_pass = progress;
            summary.battle_pass_error = None;
        }
        Err(error) => {
            summary.battle_pass = None;
            summary.battle_pass_error = Some(error);
        }
    }

    Ok(summary)
}

fn known_account_level(level: Option<i64>) -> Option<i64> {
    level.filter(|level| *level > 0)
}

async fn fetch_battle_pass_progress(
    api: &RiotApi,
    credentials: &ApiCredentials,
    metadata: &LoadoutMetadata,
) -> Result<Option<BattlePassProgressDisplay>, String> {
    let (contracts, content) =
        iced::futures::try_join!(api.contracts(credentials), api.game_content(credentials))
            .map_err(|error| error.to_string())?;

    Ok(battle_pass_progress_from_responses(
        &contracts,
        &metadata.contracts,
        Some(&content),
        &metadata.weapon_content.skins,
        &metadata.accessories,
        &metadata.currencies,
    ))
}

pub(in crate::ui) fn resolve_current_skin(
    catalog: &SkinCatalog,
    skin_id: &str,
    skin_level_id: &str,
    chroma_id: &str,
) -> ResolvedSkin {
    let mut fallback = None;

    for id in [chroma_id, skin_level_id, skin_id] {
        let skin = catalog.resolve(id);

        if skin.display_name == id {
            continue;
        }

        if skin.display_icon.is_some() {
            return skin;
        }

        fallback.get_or_insert(skin);
    }

    fallback.unwrap_or_else(|| catalog.resolve(skin_id))
}

#[cfg(test)]
mod tests {
    use crate::riot::content::Flex;

    use super::*;

    #[test]
    fn totem_rewards_resolve_to_flex_items() {
        let accessories =
            AccessoryCatalog::from_parts(vec![], vec![], vec![], vec![]).with_flex(vec![Flex {
                uuid: "flex-uuid".to_string(),
                display_name: "ORA Flex".to_string(),
                display_icon: Some("flex-icon".to_string()),
            }]);
        let reward = ResolvedContractReward {
            kind: "Totem".to_string(),
            uuid: "FLEX-UUID".to_string(),
            amount: 1,
        };

        let resolved = resolve_battle_pass_reward(
            &reward,
            &SkinCatalog::default(),
            &accessories,
            &CurrencyCatalog::default(),
        );

        assert_eq!(resolved.name, "ORA Flex");
        assert_eq!(resolved.kind, "Flex");
        assert_eq!(resolved.display_icon.as_deref(), Some("flex-icon"));
    }
}
