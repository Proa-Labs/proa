// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Team {
    pub abbreviation: String,
    pub location: String,
    pub name: String,
    pub record: String,
    pub logo: String,
    pub score: String,
    pub linescore: Vec<String>,
    pub hits: String,
    pub errors: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Batter {
    pub name: String,
    pub position: String,
    pub stats: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pitcher {
    pub name: String,
    pub note: String,
    pub stats: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Play {
    pub inning: String,
    pub text: String,
    pub score: String,
    pub scoring: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewsItem {
    pub image: String,
    pub headline: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameInfoItem {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recap {
    pub image: String,
    pub image_alt: String,
    pub category: String,
    pub headline: String,
    pub description: String,
    pub link_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopPerformer {
    pub logo: String,
    pub name: String,
    pub position: String,
    pub line: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameData {
    pub league: String,
    pub game_status: String,
    pub game_date: String,
    pub short_date: String,
    pub start_time: String,
    pub venue: String,
    pub winning_pitcher: String,
    pub losing_pitcher: String,
    pub game_information: Vec<GameInfoItem>,
    pub recap: Recap,
    pub top_performers: Vec<TopPerformer>,
    pub away: Team,
    pub home: Team,
    pub away_batting: Vec<Batter>,
    pub home_batting: Vec<Batter>,
    pub away_pitching: Vec<Pitcher>,
    pub home_pitching: Vec<Pitcher>,
    pub plays: Vec<Play>,
    pub news: Vec<NewsItem>,
}

pub static GAME: LazyLock<GameData> = LazyLock::new(|| {
    serde_json::from_str(include_str!("data/game.json"))
        .expect("embedded ESPN game fixture must be valid")
});
