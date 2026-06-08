use super::*;

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
    pub(in crate::ui) account_level: i64,
    pub(in crate::ui) gun_skins: Vec<LoadoutGunDisplay>,
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
                let weapon = WeaponDisplay::from(weapons.resolve(&gun.id));
                let base_skin = skins.resolve(&gun.skin_id);
                let skin_level = loadout_skin_level_label(
                    &skins.resolve(&gun.skin_level_id),
                    &base_skin.display_name,
                    &gun.skin_level_id,
                );
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
                }
            })
            .collect::<Vec<_>>();
        gun_skins.sort_by_key(|gun| weapon_order(&gun.weapon.display_name));

        Self {
            account_level: account_level.unwrap_or(response.identity.account_level),
            gun_skins,
            battle_pass: None,
            battle_pass_error: None,
        }
    }

    pub(in crate::ui) fn battle_pass_timer_active(&self) -> bool {
        self.battle_pass
            .as_ref()
            .is_some_and(|battle_pass| battle_pass.remaining_seconds.is_some())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct BattlePassProgressDisplay {
    pub(in crate::ui) name: String,
    pub(in crate::ui) season_name: Option<String>,
    pub(in crate::ui) level_reached: i64,
    pub(in crate::ui) total_levels: Option<i64>,
    pub(in crate::ui) progression_towards_next_level: i64,
    pub(in crate::ui) next_level_progress_required: Option<i64>,
    pub(in crate::ui) total_progression_earned: i64,
    pub(in crate::ui) total_progression_required: Option<i64>,
    pub(in crate::ui) completed: bool,
    pub(in crate::ui) remaining_seconds: Option<i64>,
    pub(in crate::ui) earned_rewards: Vec<BattlePassRewardDisplay>,
    pub(in crate::ui) unearned_rewards: Vec<BattlePassRewardDisplay>,
    pub(in crate::ui) locked_paid_rewards: Vec<BattlePassRewardDisplay>,
    pub(in crate::ui) loaded_at: iced::time::Instant,
}

impl BattlePassProgressDisplay {
    pub(in crate::ui) fn title(&self) -> String {
        self.season_name
            .as_ref()
            .filter(|name| !name.trim().is_empty())
            .map(|season| format!("{season} Battle Pass"))
            .unwrap_or_else(|| self.name.clone())
    }

    pub(in crate::ui) fn tier_label(&self) -> String {
        match self.total_levels {
            Some(total_levels) if total_levels > 0 => {
                format!(
                    "Tier {} of {}",
                    self.level_reached.clamp(0, total_levels),
                    total_levels
                )
            }
            _ => format!("Tier {}", self.level_reached.max(0)),
        }
    }

    pub(in crate::ui) fn next_tier_label(&self) -> String {
        if self.completed {
            return "Complete".to_string();
        }

        match self.next_level_progress_required {
            Some(required) if required > 0 => format!(
                "{} / {} XP toward next tier",
                format_whole_number(self.progression_towards_next_level.max(0)),
                format_whole_number(required)
            ),
            _ => format!(
                "{} XP toward next tier",
                format_whole_number(self.progression_towards_next_level.max(0))
            ),
        }
    }

    pub(in crate::ui) fn progress_percent_label(&self) -> Option<String> {
        self.total_progression_required
            .filter(|required| *required > 0)
            .map(|required| {
                let percent =
                    (self.total_progression_earned.max(0) as f64 / required as f64) * 100.0;
                format!("{:.0}% complete", percent.clamp(0.0, 100.0))
            })
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::ui) enum BattlePassRewardTrack {
    Free,
    Paid,
}

impl BattlePassRewardTrack {
    pub(in crate::ui) fn label(self) -> &'static str {
        match self {
            Self::Free => "Free",
            Self::Paid => "Paid",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct BattlePassRewardDisplay {
    pub(in crate::ui) tier: i64,
    pub(in crate::ui) chapter: i64,
    pub(in crate::ui) level_in_chapter: i64,
    pub(in crate::ui) is_epilogue: bool,
    pub(in crate::ui) track: BattlePassRewardTrack,
    pub(in crate::ui) uuid: String,
    pub(in crate::ui) name: String,
    pub(in crate::ui) kind: String,
    pub(in crate::ui) amount: i64,
    pub(in crate::ui) highlighted: bool,
    pub(in crate::ui) display_icon: Option<String>,
    pub(in crate::ui) viewer_icon: Option<String>,
    pub(in crate::ui) cached_icon: Option<PathBuf>,
}

impl BattlePassRewardDisplay {
    pub(in crate::ui) fn location_label(&self) -> String {
        if self.is_epilogue {
            format!("Epilogue tier {}", self.tier.max(0))
        } else {
            format!(
                "Tier {} (Ch {} L{})",
                self.tier.max(0),
                self.chapter.max(0),
                self.level_in_chapter.max(0)
            )
        }
    }

    pub(in crate::ui) fn amount_label(&self) -> Option<String> {
        (self.kind != "Currency" && self.amount > 1)
            .then(|| format!("x{}", format_whole_number(self.amount)))
    }
}

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
    let progression_deltas = definition.level_xp.as_slice();
    let total_levels = Some(i64::try_from(progression_deltas.len()).unwrap_or(0));
    let total_progression_required = Some(progression_deltas.iter().copied().sum::<i64>());
    let next_level_index = usize::try_from(contract.progression_level_reached.max(0)).ok();
    let next_level_progress_required =
        next_level_index.and_then(|index| progression_deltas.get(index).copied());
    let completed = contract.progression_completed
        || total_levels
            .is_some_and(|levels| levels > 0 && contract.progression_level_reached >= levels);
    let remaining_seconds =
        active_act.and_then(|act| remaining_seconds_until_utc_at(&act.end_time, context.now_utc));
    let paid_pass_owned = battle_pass_paid_pass_owned(definition, contract);
    let (earned_rewards, unearned_rewards, locked_paid_rewards) = battle_pass_reward_groups(
        definition,
        contract,
        paid_pass_owned,
        context.skins,
        context.accessories,
        context.currencies,
    );

    Some(BattlePassProgressDisplay {
        name: non_empty_string(definition.display_name.clone())
            .unwrap_or_else(|| "Battle Pass".to_string()),
        season_name: active_act.and_then(|act| non_empty_string(act.name.clone())),
        level_reached: contract.progression_level_reached,
        total_levels,
        progression_towards_next_level: contract.progression_towards_next_level,
        next_level_progress_required,
        total_progression_earned: contract.contract_progression.total_progression_earned,
        total_progression_required,
        completed,
        remaining_seconds,
        earned_rewards,
        unearned_rewards,
        locked_paid_rewards,
        loaded_at: context.loaded_at,
    })
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
            ids_match(reward_schedule_id, schedule_id) && level.amount > 0
        })
}

fn battle_pass_reward_groups(
    definition: &ResolvedContract,
    contract: &PlayerContract,
    paid_pass_owned: bool,
    skins: &SkinCatalog,
    accessories: &AccessoryCatalog,
    currencies: &CurrencyCatalog,
) -> (
    Vec<BattlePassRewardDisplay>,
    Vec<BattlePassRewardDisplay>,
    Vec<BattlePassRewardDisplay>,
) {
    let mut earned_rewards = Vec::new();
    let mut unearned_rewards = Vec::new();
    let mut locked_paid_rewards = Vec::new();
    let level_reached = contract.progression_level_reached.max(0);

    for level in &definition.reward_levels {
        if let Some(reward) = &level.premium_reward {
            let display = battle_pass_reward_display(
                reward,
                level,
                BattlePassRewardTrack::Paid,
                skins,
                accessories,
                currencies,
            );

            if !paid_pass_owned {
                locked_paid_rewards.push(display);
            } else if level.tier <= level_reached {
                earned_rewards.push(display);
            } else {
                unearned_rewards.push(display);
            }
        }

        for reward in &level.free_rewards {
            let display = battle_pass_reward_display(
                reward,
                level,
                BattlePassRewardTrack::Free,
                skins,
                accessories,
                currencies,
            );

            if level.tier <= level_reached {
                earned_rewards.push(display);
            } else {
                unearned_rewards.push(display);
            }
        }
    }

    (earned_rewards, unearned_rewards, locked_paid_rewards)
}

fn battle_pass_reward_display(
    reward: &ResolvedContractReward,
    level: &crate::riot::content::ResolvedContractRewardLevel,
    track: BattlePassRewardTrack,
    skins: &SkinCatalog,
    accessories: &AccessoryCatalog,
    currencies: &CurrencyCatalog,
) -> BattlePassRewardDisplay {
    let resolved = resolve_battle_pass_reward(reward, skins, accessories, currencies);

    BattlePassRewardDisplay {
        tier: level.tier,
        chapter: level.chapter,
        level_in_chapter: level.level_in_chapter,
        is_epilogue: level.is_epilogue,
        track,
        uuid: reward.uuid.clone(),
        name: resolved.name,
        kind: resolved.kind,
        amount: reward.amount,
        highlighted: reward.highlighted,
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
                kind: "Weapon skin".to_string(),
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
                .find(|contract| ids_match(&contract.contract_definition_id, &definition.uuid))
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

fn ids_match(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

fn remaining_seconds_until_utc_at(end_time: &str, now: OffsetDateTime) -> Option<i64> {
    let end = parse_utc_timestamp(end_time)?;

    Some(
        end.unix_timestamp()
            .saturating_sub(now.unix_timestamp())
            .max(0),
    )
}

fn parse_utc_timestamp(value: &str) -> Option<OffsetDateTime> {
    let trimmed = value.trim();
    let timestamp = trimmed
        .strip_suffix('Z')
        .or_else(|| trimmed.strip_suffix("+00:00"))
        .unwrap_or(trimmed);
    let (date, time) = timestamp.split_once('T')?;
    let mut date_parts = date.split('-');
    let year = date_parts.next()?.parse::<i32>().ok()?;
    let month = date_parts.next()?.parse::<u8>().ok()?;
    let day = date_parts.next()?.parse::<u8>().ok()?;

    if date_parts.next().is_some() {
        return None;
    }

    let mut time_parts = time.split(':');
    let hour = time_parts.next()?.parse::<u8>().ok()?;
    let minute = time_parts.next()?.parse::<u8>().ok()?;
    let second_part = time_parts.next()?;

    if time_parts.next().is_some() {
        return None;
    }

    let second = second_part
        .split_once('.')
        .map(|(seconds, _)| seconds)
        .unwrap_or(second_part)
        .parse::<u8>()
        .ok()?;
    let date = Date::from_calendar_date(year, Month::try_from(month).ok()?, day).ok()?;
    let time = Time::from_hms(hour, minute, second).ok()?;

    Some(PrimitiveDateTime::new(date, time).assume_utc())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct LoadoutGunDisplay {
    pub(in crate::ui) weapon: WeaponDisplay,
    pub(in crate::ui) skin: SkinDisplay,
    pub(in crate::ui) skin_name: String,
    pub(in crate::ui) skin_level: Option<String>,
}

impl LoadoutGunDisplay {
    pub(in crate::ui) fn skin_detail_label(&self) -> String {
        match self
            .skin_level
            .as_ref()
            .map(|level| level.trim())
            .filter(|level| !level.is_empty())
        {
            Some(level) => format!("{} - {}", self.skin_name, level),
            None => self.skin_name.clone(),
        }
    }

    #[cfg(test)]
    pub(in crate::ui) fn label(&self) -> String {
        format!("{}: {}", self.weapon.display_name, self.skin_detail_label())
    }
}

fn loadout_skin_level_label(
    level: &ResolvedSkin,
    skin_name: &str,
    level_id: &str,
) -> Option<String> {
    level.level_label.clone().or_else(|| {
        let display_name = level.display_name.trim();

        (!display_name.is_empty()
            && !display_name.eq_ignore_ascii_case(level_id)
            && !display_name.eq_ignore_ascii_case(skin_name))
        .then(|| display_name.to_string())
    })
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

pub(in crate::ui) fn weapon_category(name: &str) -> &'static str {
    match name {
        "Classic" | "Shorty" | "Frenzy" | "Ghost" | "Sheriff" | "Bandit" => "Sidearms",
        "Stinger" | "Spectre" => "SMGs",
        "Bucky" | "Judge" => "Shotguns",
        "Bulldog" | "Guardian" | "Phantom" | "Vandal" => "Rifles",
        "Marshal" | "Outlaw" | "Operator" => "Sniper Rifles",
        "Ares" | "Odin" => "Heavy",
        "Melee" => "Melee",
        _ => "Other",
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct WeaponDisplay {
    pub(in crate::ui) uuid: String,
    pub(in crate::ui) display_name: String,
    pub(in crate::ui) display_icon: Option<String>,
    pub(in crate::ui) viewer_icon: Option<String>,
    pub(in crate::ui) cached_icon: Option<PathBuf>,
}

impl From<ResolvedWeapon> for WeaponDisplay {
    fn from(weapon: ResolvedWeapon) -> Self {
        Self {
            uuid: weapon.uuid,
            display_name: weapon.display_name,
            display_icon: weapon.display_icon,
            viewer_icon: weapon.viewer_icon,
            cached_icon: None,
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
            cached_icon: None,
        }
    }
}

pub(in crate::ui) async fn fetch_loadout(
    account: AccountProfile,
    client_version: String,
    image_cache: ImageCache,
) -> Result<LoadoutResult, String> {
    let api = RiotApi::new().map_err(|error| error.to_string())?;
    let resolved = resolve_credentials(&api, &account, client_version).await?;
    let metadata = fetch_loadout_metadata().await?;
    let account_level = api
        .account_xp(&resolved.credentials)
        .await
        .ok()
        .map(|xp| xp.progress.level);
    let battle_pass = fetch_battle_pass_progress(&api, &resolved.credentials, &metadata).await;
    let mut summary = api
        .player_loadout(&resolved.credentials)
        .await
        .map(|response| {
            LoadoutSummary::from_response(
                response,
                &metadata.skins,
                &metadata.weapons,
                account_level,
            )
        })
        .map_err(|error| error.to_string())?;
    match battle_pass {
        Ok(progress) => {
            summary.battle_pass = Some(progress);
            summary.battle_pass_error = None;
        }
        Err(error) => {
            summary.battle_pass = None;
            summary.battle_pass_error = Some(error);
        }
    }
    cache_loadout_images(&mut summary, &image_cache).await?;

    Ok(LoadoutResult {
        account_id: account.id,
        summary,
        session: resolved.session,
        launcher_session: resolved.launcher_session,
        identity: resolved.identity,
    })
}

async fn fetch_battle_pass_progress(
    api: &RiotApi,
    credentials: &ApiCredentials,
    metadata: &LoadoutMetadata,
) -> Result<BattlePassProgressDisplay, String> {
    let contracts = api
        .contracts(credentials)
        .await
        .map_err(|error| error.to_string())?;
    let content = api
        .game_content(credentials)
        .await
        .map_err(|error| error.to_string())?;

    battle_pass_progress_from_responses(
        &contracts,
        &metadata.contracts,
        Some(&content),
        &metadata.skins,
        &metadata.accessories,
        &metadata.currencies,
    )
    .ok_or_else(|| "No active battle pass progress found".to_string())
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
