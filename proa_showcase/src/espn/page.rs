// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::fixture::{
    Batter, GameData, GameInfoItem, NewsItem, Pitcher, Play, Team, TopPerformer, GAME,
};
use super::CSS;
use proa_core::{
    DataLoader, RenderBuffer, StaticRaw, WebContext, WebRenderSync, WriteBuf, WriteError,
};
use proa_macros::html_sync;
use rsjs::{event_handler, signal};
use rsjs_macros::rsjs;

pub fn render() -> String {
    let mut out = RenderBuffer::new();
    let (artifacts, nonce) = {
        let mut cx = WebContext::with_buffer(&mut out);
        render_into(&mut cx, &GAME).expect("ESPN showcase page should render");
        (cx.rsjs_artifacts_snapshot(), cx.csp_nonce_cloned())
    };
    let mut html = out.into_string();
    crate::finalize_rsjs_document(&mut html, &artifacts, nonce.as_ref())
        .expect("ESPN rsjs runtime plan should render");
    html
}

pub fn render_into<B: WriteBuf, L: DataLoader>(
    cx: &mut WebContext<L, B>,
    data: &GameData,
) -> Result<(), WriteError> {
    render_into_with_overrides(
        cx,
        data,
        EspnPageOverrides {
            away_score: &data.away.score,
            home_score: &data.home.score,
            game_status: &data.game_status,
            game_date: &data.game_date,
            venue: &data.venue,
        },
    )
}

pub fn render_into_with_overrides<B: WriteBuf, L: DataLoader>(
    cx: &mut WebContext<L, B>,
    data: &GameData,
    overrides: EspnPageOverrides<'_>,
) -> Result<(), WriteError> {
    EspnGamePage { data, overrides }.render(cx)
}

#[derive(Debug, Clone, Copy)]
pub struct EspnPageOverrides<'a> {
    pub away_score: &'a str,
    pub home_score: &'a str,
    pub game_status: &'a str,
    pub game_date: &'a str,
    pub venue: &'a str,
}

impl Default for EspnPageOverrides<'static> {
    fn default() -> Self {
        Self {
            away_score: "2",
            home_score: "18",
            game_status: "Final",
            game_date: "May 31, 2025",
            venue: "Dodger Stadium · Los Angeles, CA",
        }
    }
}

pub struct EspnGamePage<'a> {
    pub data: &'a GameData,
    pub overrides: EspnPageOverrides<'a>,
}

// Preserve the public names used by the previous generic sports fixture while
// the restored showcase exposes its ESPN-specific names.
pub type SportsPageOverrides<'a> = EspnPageOverrides<'a>;
pub type SportsGamePage<'a> = EspnGamePage<'a>;

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for EspnGamePage<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <!DOCTYPE html>
            <html lang="en">
                <head>
                    <meta charset="utf-8" />
                    <meta name="viewport" content="width=device-width, initial-scale=1" />
                    <meta name="robots" content="noindex, nofollow" />
                    <title>"Yankees at Dodgers Box Score - ESPN"</title>
                    <style>{StaticRaw(CSS)}</style>
                </head>
                <body>
                    {TopChrome { data: self.data }}
                    <div class="espn-ad espn-ad--top">
                        <span>"ADVERTISEMENT"</span>
                    </div>
                    <main class="espn-page">
                        {Scoreboard {
                            league: &self.data.league,
                            away: &self.data.away,
                            home: &self.data.home,
                            short_date: &self.data.short_date,
                            start_time: &self.data.start_time,
                            winning_pitcher: &self.data.winning_pitcher,
                            losing_pitcher: &self.data.losing_pitcher,
                            overrides: self.overrides,
                        }}
                        {GameTabs { data: self.data }}
                    </main>
                    {Footer}
                </body>
            </html>
        }
        .render(cx)
    }
}

struct TopChrome<'a> {
    data: &'a GameData,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for TopChrome<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <header class="espn-header">
                <div class="espn-header__inner">
                    <button class="espn-menu" type="button" aria-label="Open menu">
                        <i></i>
                        <i></i>
                        <i></i>
                    </button>
                    <a class="espn-logo" href="#" aria-label="ESPN">"ESPN"</a>
                    <nav class="espn-global" aria-label="ESPN sections">
                        <a href="#">"NFL"</a>
                        <a href="#">"NBA"</a>
                        <a href="#">"MLB"</a>
                        <a href="#">"NHL"</a>
                        <a href="#">"Soccer"</a>
                        <a href="#">
                            "More"
                            <span>"⌄"</span>
                        </a>
                    </nav>
                    <div class="espn-account">
                        <button type="button" aria-label="Search">"⌕"</button>
                        <button type="button">"Log In"</button>
                    </div>
                </div>
            </header>
            <div class="espn-leaguebar">
                <div class="espn-leaguebar__inner">
                    <strong>{self.data.league.as_str()}</strong>
                    <a href="#">"Home"</a>
                    <a href="#">"Scores"</a>
                    <a href="#">"Schedule"</a>
                    <a href="#">"Standings"</a>
                    <a href="#">"Stats"</a>
                    <a href="#">"Teams"</a>
                    <a href="#">"Transactions"</a>
                </div>
            </div>
            <section class="score-strip" aria-label="Other MLB scores">
                <div class="score-strip__inner">
                    <button type="button" aria-label="Previous scores">"‹"</button>
                    <div class="score-tile">
                        <span>"FINAL"</span>
                        <b>"BOS"</b>
                        <strong>"4"</strong>
                        <b>"ATL"</b>
                        <strong>"2"</strong>
                    </div>
                    <div class="score-tile">
                        <span>"FINAL"</span>
                        <b>"CHC"</b>
                        <strong>"1"</strong>
                        <b>"CIN"</b>
                        <strong>"6"</strong>
                    </div>
                    <div class="score-tile score-tile--active">
                        <span>"FINAL"</span>
                        <b>{self.data.away.abbreviation.as_str()}</b>
                        <strong>{self.data.away.score.as_str()}</strong>
                        <b>{self.data.home.abbreviation.as_str()}</b>
                        <strong>{self.data.home.score.as_str()}</strong>
                    </div>
                    <div class="score-tile">
                        <span>"FINAL"</span>
                        <b>"SF"</b>
                        <strong>"3"</strong>
                        <b>"MIA"</b>
                        <strong>"4"</strong>
                    </div>
                    <div class="score-tile">
                        <span>"FINAL"</span>
                        <b>"MIN"</b>
                        <strong>"2"</strong>
                        <b>"SEA"</b>
                        <strong>"5"</strong>
                    </div>
                    <button type="button" aria-label="Next scores">"›"</button>
                </div>
            </section>
        }
        .render(cx)
    }
}

struct Scoreboard<'a> {
    league: &'a str,
    away: &'a Team,
    home: &'a Team,
    short_date: &'a str,
    start_time: &'a str,
    winning_pitcher: &'a str,
    losing_pitcher: &'a str,
    overrides: EspnPageOverrides<'a>,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for Scoreboard<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <section class="matchup-card" data-espn-scoreboard>
                <div class="matchup-card__meta">
                    <span>{self.league}</span>
                    <time>{self.overrides.game_date}</time>
                    <span>{self.overrides.venue}</span>
                </div>
                <div class="matchup-card__body">
                    <div class="team-block team-block--away">
                        <img src={self.away.logo.as_str()} alt={self.away.name.as_str()} />
                        <div>
                            <p>{self.away.location.as_str()}</p>
                            <h1>{self.away.name.as_str()}</h1>
                            <span>{self.away.record.as_str()}</span>
                        </div>
                        <strong>{self.overrides.away_score}</strong>
                    </div>
                    <div class="game-status">
                        <b>{self.overrides.game_status}</b>
                        <span>
                            "R"
                            <i>"H"</i>
                            <i>"E"</i>
                        </span>
                        <p>
                            {self.short_date}
                            <br />
                            {self.start_time}
                        </p>
                    </div>
                    <div class="team-block team-block--home">
                        <strong>{self.overrides.home_score}</strong>
                        <div>
                            <p>{self.home.location.as_str()}</p>
                            <h1>{self.home.name.as_str()}</h1>
                            <span>{self.home.record.as_str()}</span>
                        </div>
                        <img src={self.home.logo.as_str()} alt={self.home.name.as_str()} />
                    </div>
                </div>
                {LineScore {
                    away: self.away,
                    home: self.home,
                    away_score: self.overrides.away_score,
                    home_score: self.overrides.home_score,
                }}
                <div class="matchup-card__footer">
                    <div>
                        <b>"W: "</b>
                        {self.winning_pitcher}
                    </div>
                    <div>
                        <b>"L: "</b>
                        {self.losing_pitcher}
                    </div>
                    <a href="#recap">"Recap"</a>
                </div>
            </section>
        }
        .render(cx)
    }
}

struct LineScore<'a> {
    away: &'a Team,
    home: &'a Team,
    away_score: &'a str,
    home_score: &'a str,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for LineScore<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <div class="line-score-wrap">
                <table class="line-score">
                    <thead>
                        <tr>
                            <th></th>
                            <th>"1"</th>
                            <th>"2"</th>
                            <th>"3"</th>
                            <th>"4"</th>
                            <th>"5"</th>
                            <th>"6"</th>
                            <th>"7"</th>
                            <th>"8"</th>
                            <th>"9"</th>
                            <th>"R"</th>
                            <th>"H"</th>
                            <th>"E"</th>
                        </tr>
                    </thead>
                    <tbody>
                        <tr>
                            <th>{self.away.abbreviation.as_str()}</th>
                            <td>{self.away.linescore[0].as_str()}</td>
                            <td>{self.away.linescore[1].as_str()}</td>
                            <td>{self.away.linescore[2].as_str()}</td>
                            <td>{self.away.linescore[3].as_str()}</td>
                            <td>{self.away.linescore[4].as_str()}</td>
                            <td>{self.away.linescore[5].as_str()}</td>
                            <td>{self.away.linescore[6].as_str()}</td>
                            <td>{self.away.linescore[7].as_str()}</td>
                            <td>{self.away.linescore[8].as_str()}</td>
                            <td class="total">{self.away_score}</td>
                            <td>{self.away.hits.as_str()}</td>
                            <td>{self.away.errors.as_str()}</td>
                        </tr>
                        <tr>
                            <th>{self.home.abbreviation.as_str()}</th>
                            <td>{self.home.linescore[0].as_str()}</td>
                            <td>{self.home.linescore[1].as_str()}</td>
                            <td>{self.home.linescore[2].as_str()}</td>
                            <td>{self.home.linescore[3].as_str()}</td>
                            <td>{self.home.linescore[4].as_str()}</td>
                            <td>{self.home.linescore[5].as_str()}</td>
                            <td>{self.home.linescore[6].as_str()}</td>
                            <td>{self.home.linescore[7].as_str()}</td>
                            <td>{self.home.linescore[8].as_str()}</td>
                            <td class="total">{self.home_score}</td>
                            <td>{self.home.hits.as_str()}</td>
                            <td>{self.home.errors.as_str()}</td>
                        </tr>
                    </tbody>
                </table>
            </div>
        }
        .render(cx)
    }
}

pub struct GameTabs<'a> {
    pub data: &'a GameData,
}

#[rsjs(component, client)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for GameTabs<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let active_tab = signal("box");
        let show_gamecast =
            event_handler(|_: rsjs::MouseEvent<rsjs::elem::Button>| active_tab.set("gamecast"));
        let show_box =
            event_handler(|_: rsjs::MouseEvent<rsjs::elem::Button>| active_tab.set("box"));
        let show_plays =
            event_handler(|_: rsjs::MouseEvent<rsjs::elem::Button>| active_tab.set("plays"));
        let gamecast_panel = GamecastPanel { data: self.data };
        let box_score_panel = BoxScorePanel { data: self.data };
        let play_by_play_panel = PlayByPlayPanel {
            plays: &self.data.plays,
            game_status: &self.data.game_status,
        };
        let sidebar = Sidebar {
            performers: &self.data.top_performers,
            news: &self.data.news,
        };

        html_sync! {
            <div class="game-tabs-shell">
                <nav class="game-tabs" aria_label="Game details">
                    <button
                        type="button"
                        aria_selected={active_tab.get() == "gamecast"}
                        on_click={show_gamecast()}
                    >
                        "Gamecast"
                    </button>
                    <button
                        type="button"
                        aria_selected={active_tab.get() == "box"}
                        on_click={show_box()}
                    >
                        "Box Score"
                    </button>
                    <button
                        type="button"
                        aria_selected={active_tab.get() == "plays"}
                        on_click={show_plays()}
                    >
                        "Play-by-Play"
                    </button>
                    <button type="button" aria_selected="false">"Videos"</button>
                </nav>
                <div class="espn-layout">
                    <div class="espn-primary">
                        <section hidden={active_tab.get()!= "gamecast"} data_panel="gamecast">
                            {gamecast_panel}
                        </section>
                        <section hidden={active_tab.get()!= "box"} data_panel="box-score">
                            {box_score_panel}
                        </section>
                        <section hidden={active_tab.get()!= "plays"} data_panel="play-by-play">
                            {play_by_play_panel}
                        </section>
                    </div>
                    {sidebar}
                </div>
            </div>
        }
        .render(cx)
    }
}

struct BoxScorePanel<'a> {
    data: &'a GameData,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for BoxScorePanel<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <div class="section-title">
                <h2>"Box Score"</h2>
                <span>{self.data.game_status.as_str()}</span>
            </div>
            {TeamBox {
                team: &self.data.away,
                batting: &self.data.away_batting,
                pitching: &self.data.away_pitching,
            }}
            {TeamBox {
                team: &self.data.home,
                batting: &self.data.home_batting,
                pitching: &self.data.home_pitching,
            }}
            {GameInformation { items: &self.data.game_information }}
        }
        .render(cx)
    }
}

struct TeamBox<'a> {
    team: &'a Team,
    batting: &'a [Batter],
    pitching: &'a [Pitcher],
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for TeamBox<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <article class="stat-card">
                <header class="stat-card__team">
                    <img src={self.team.logo.as_str()} alt="" />
                    <div>
                        <b>{self.team.location.as_str()}" "{self.team.name.as_str()}</b>
                        <span>{self.team.record.as_str()}</span>
                    </div>
                    <strong>{self.team.score.as_str()}</strong>
                </header>
                <div class="table-scroll">
                    <table class="player-table batting-table">
                        <caption>{self.team.name.as_str()}" Batting"</caption>
                        <thead>
                            <tr>
                                <th>"Hitters"</th>
                                <th>"H-AB"</th>
                                <th>"AB"</th>
                                <th>"R"</th>
                                <th>"H"</th>
                                <th>"RBI"</th>
                                <th>"HR"</th>
                                <th>"BB"</th>
                                <th>"K"</th>
                            </tr>
                        </thead>
                        <tbody>{BatterRows { players: self.batting }}</tbody>
                        <tfoot>
                            <tr>
                                <th>"Totals"</th>
                                <td></td>
                                <td>"38"</td>
                                <td>{self.team.score.as_str()}</td>
                                <td>{self.team.hits.as_str()}</td>
                                <td>{if self.team.abbreviation == "LAD" { "18" } else { "2" }}</td>
                                <td>{if self.team.abbreviation == "LAD" { "5" } else { "2" }}</td>
                                <td>"6"</td>
                                <td>"10"</td>
                            </tr>
                        </tfoot>
                    </table>
                </div>
                <div class="stat-notes">
                    <b>"Batting"</b>
                    <p>
                        "2B: "
                        {if self.team.abbreviation == "LAD" { "Edman 2 (15), Freeman (18), Smith (9)" } else { "Judge (13)" }}
                    </p>
                    <p>
                        "HR: "
                        {if self.team.abbreviation == "LAD" { "Muncy 2 (7), Kim (3), Pages (11), Rushing (2)" } else { "Judge 2 (21)" }}
                    </p>
                </div>
                <div class="table-scroll">
                    <table class="player-table pitching-table">
                        <caption>{self.team.name.as_str()}" Pitching"</caption>
                        <thead>
                            <tr>
                                <th>"Pitchers"</th>
                                <th>"IP"</th>
                                <th>"H"</th>
                                <th>"R"</th>
                                <th>"ER"</th>
                                <th>"BB"</th>
                                <th>"K"</th>
                                <th>"HR"</th>
                                <th>"PC-ST"</th>
                                <th>"ERA"</th>
                            </tr>
                        </thead>
                        <tbody>{PitcherRows { players: self.pitching }}</tbody>
                        <tfoot>
                            <tr>
                                <th>"Totals"</th>
                                <td>"8.0"</td>
                                <td>{if self.team.abbreviation == "LAD" { "7" } else { "21" }}</td>
                                <td>{if self.team.abbreviation == "LAD" { "2" } else { "18" }}</td>
                                <td>{if self.team.abbreviation == "LAD" { "2" } else { "18" }}</td>
                                <td>"6"</td>
                                <td>"10"</td>
                                <td>{if self.team.abbreviation == "LAD" { "2" } else { "5" }}</td>
                                <td>"187-114"</td>
                                <td></td>
                            </tr>
                        </tfoot>
                    </table>
                </div>
            </article>
        }
        .render(cx)
    }
}

struct BatterRows<'a> {
    players: &'a [Batter],
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for BatterRows<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        for player in self.players {
            html_sync! {
                <tr>
                    <th>
                        <a href="#player">{player.name.as_str()}</a>
                        <span>{player.position.as_str()}</span>
                    </th>
                    <td>{player.stats[0].as_str()}</td>
                    <td>{player.stats[1].as_str()}</td>
                    <td>{player.stats[2].as_str()}</td>
                    <td>{player.stats[3].as_str()}</td>
                    <td>{player.stats[4].as_str()}</td>
                    <td>{player.stats[5].as_str()}</td>
                    <td>{player.stats[6].as_str()}</td>
                    <td>{player.stats[7].as_str()}</td>
                </tr>
            }
            .render(cx)?;
        }
        Ok(())
    }
}

struct PitcherRows<'a> {
    players: &'a [Pitcher],
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for PitcherRows<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        for player in self.players {
            html_sync! {
                <tr>
                    <th>
                        <a href="#player">{player.name.as_str()}</a>
                        <span>{player.note.as_str()}</span>
                    </th>
                    <td>{player.stats[0].as_str()}</td>
                    <td>{player.stats[1].as_str()}</td>
                    <td>{player.stats[2].as_str()}</td>
                    <td>{player.stats[3].as_str()}</td>
                    <td>{player.stats[4].as_str()}</td>
                    <td>{player.stats[5].as_str()}</td>
                    <td>{player.stats[6].as_str()}</td>
                    <td>{player.stats[7].as_str()}</td>
                    <td>{player.stats[8].as_str()}</td>
                </tr>
            }
            .render(cx)?;
        }
        Ok(())
    }
}

struct GamecastPanel<'a> {
    data: &'a GameData,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for GamecastPanel<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <div class="section-title">
                <h2>"Gamecast"</h2>
                <span>{self.data.game_status.as_str()}</span>
            </div>
            <article class="recap-card" id="recap">
                <div class="recap-card__image">
                    <img
                        src={self.data.recap.image.as_str()}
                        alt={self.data.recap.image_alt.as_str()}
                    />
                    <i>"▶"</i>
                </div>
                <div class="recap-card__copy">
                    <span>{self.data.recap.category.as_str()}</span>
                    <h3>{self.data.recap.headline.as_str()}</h3>
                    <p>{self.data.recap.description.as_str()}</p>
                    <a href="#">{self.data.recap.link_label.as_str()}</a>
                </div>
            </article>
            <article class="panel-card win-card">
                <header>
                    <h3>"Win Probability"</h3>
                    <button type="button">"Game Flow"</button>
                </header>
                <div class="win-legend">
                    <span>
                        <i class="nyy-dot"></i>
                        {self.data.away.abbreviation.as_str()}
                        " 0%"
                    </span>
                    <span>
                        <i class="lad-dot"></i>
                        {self.data.home.abbreviation.as_str()}
                        " 100%"
                    </span>
                </div>
                <svg
                    viewBox="0 0 700 230"
                    role="img"
                    aria-label="Dodgers win probability rose from 52 to 100 percent"
                >
                    <line x1="32" y1="18" x2="32" y2="194" />
                    <line x1="32" y1="194" x2="682" y2="194" />
                    <line x1="32" y1="106" x2="682" y2="106" class="mid" />
                    <polyline points="32,103 74,40 108,33 146,25 184,27 226,20 270,19 312,17 358,15 402,14 452,13 500,12 550,11 608,10 682,9" />
                    <text x="25" y="213">"1"</text>
                    <text x="100" y="213">"2"</text>
                    <text x="177" y="213">"3"</text>
                    <text x="254" y="213">"4"</text>
                    <text x="331" y="213">"5"</text>
                    <text x="408" y="213">"6"</text>
                    <text x="485" y="213">"7"</text>
                    <text x="562" y="213">"8"</text>
                    <text x="664" y="213">"9"</text>
                </svg>
            </article>
            <article class="panel-card scoring-card">
                <header>
                    <h3>"Scoring Summary"</h3>
                </header>
                {PlayList { plays: &self.data.plays, scoring_only: true }}
            </article>
        }
        .render(cx)
    }
}

struct PlayByPlayPanel<'a> {
    plays: &'a [Play],
    game_status: &'a str,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for PlayByPlayPanel<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <div class="section-title">
                <h2>"Play-by-Play"</h2>
                <span>{self.game_status}</span>
            </div>
            <article class="panel-card plays-card">
                <header>
                    <h3>"All Plays"</h3>
                    <select aria-label="Select inning">
                        <option>"All Innings"</option>
                        <option>"9th Inning"</option>
                    </select>
                </header>
                {PlayList { plays: self.plays, scoring_only: false }}
            </article>
        }
        .render(cx)
    }
}

struct PlayList<'a> {
    plays: &'a [Play],
    scoring_only: bool,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for PlayList<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let mut previous = "";
        for play in self.plays {
            if self.scoring_only && !play.scoring {
                continue;
            }
            if play.inning != previous {
                html_sync! {
                    <h4 class="inning-heading">{play.inning.as_str()}</h4>
                }
                .render(cx)?;
                previous = play.inning.as_str();
            }
            html_sync! {
                <div class="play-row">
                    <span class="play-icon">{if play.scoring { "⚾" } else { "•" }}</span>
                    <p>{play.text.as_str()}</p>
                    <strong>{play.score.as_str()}</strong>
                </div>
            }
            .render(cx)?;
        }
        Ok(())
    }
}

struct GameInformation<'a> {
    items: &'a [GameInfoItem],
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for GameInformation<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <article class="panel-card game-info">
                <header>
                    <h3>"Game Information"</h3>
                </header>
                <img src="/images/espn/dodger-stadium.jpg" alt="Dodger Stadium" />
                <dl>{GameInfoRows { items: self.items }}</dl>
            </article>
        }
        .render(cx)
    }
}

struct GameInfoRows<'a> {
    items: &'a [GameInfoItem],
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for GameInfoRows<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        for item in self.items {
            html_sync! {
                <div>
                    <dt>{item.label.as_str()}</dt>
                    <dd>{item.value.as_str()}</dd>
                </div>
            }
            .render(cx)?;
        }
        Ok(())
    }
}

struct Sidebar<'a> {
    performers: &'a [TopPerformer],
    news: &'a [NewsItem],
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for Sidebar<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <aside class="espn-sidebar">
                <div class="espn-ad espn-ad--box">
                    <span>"ADVERTISEMENT"</span>
                </div>
                <article class="panel-card top-performers">
                    <header>
                        <h3>"Top Performers"</h3>
                    </header>
                    {TopPerformerRows { performers: self.performers }}
                </article>
                <article class="panel-card news-card">
                    <header>
                        <h3>"MLB News"</h3>
                    </header>
                    {NewsRows { news: self.news }}
                    <a class="more-news" href="#">"All MLB News →"</a>
                </article>
            </aside>
        }
        .render(cx)
    }
}

struct TopPerformerRows<'a> {
    performers: &'a [TopPerformer],
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for TopPerformerRows<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        for performer in self.performers {
            html_sync! {
                <div>
                    <img src={performer.logo.as_str()} alt="" />
                    <p>
                        <b>{performer.name.as_str()}</b>
                        <span>{performer.position.as_str()}</span>
                    </p>
                    <strong>
                        {performer.line.as_str()}
                        <small>{performer.note.as_str()}</small>
                    </strong>
                </div>
            }
            .render(cx)?;
        }
        Ok(())
    }
}

struct NewsRows<'a> {
    news: &'a [NewsItem],
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for NewsRows<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        for item in self.news {
            html_sync! {
                <a class="news-row" href="#">
                    <img src={item.image.as_str()} alt="" />
                    <span>
                        <b>{item.headline.as_str()}</b>
                        <small>{item.description.as_str()}</small>
                    </span>
                </a>
            }
            .render(cx)?;
        }
        Ok(())
    }
}

struct Footer;

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for Footer {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <footer class="espn-footer">
                <div>
                    <b class="espn-logo">"ESPN"</b>
                    <nav>
                        <a href="#">"Terms of Use"</a>
                        <a href="#">"Privacy Policy"</a>
                        <a href="#">"Your US State Privacy Rights"</a>
                        <a href="#">"Children's Online Privacy Policy"</a>
                    </nav>
                    <p>"© 2026 ESPN Enterprises, Inc. All rights reserved."</p>
                </div>
            </footer>
        }
        .render(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_dense_box_score_and_rsjs_markers() {
        let html = render();
        assert!(html.contains("data-espn-scoreboard"));
        assert!(html.contains("data-rsjs-island-root"));
        assert!(html.contains("/rsjs-runtime.js"));
        assert_eq!(
            html.matches(r#"<script type="module" src="/rsjs-runtime.js""#)
                .count(),
            1
        );
        assert!(
            html.find("/rsjs-runtime.js").unwrap() < html.rfind("</body>").unwrap(),
            "runtime plan must remain inside the document body"
        );
        assert!(html.len() > 25_000);
    }

    #[test]
    fn renders_playground_overrides() {
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);
        render_into_with_overrides(
            &mut cx,
            &GAME,
            EspnPageOverrides {
                away_score: "7",
                home_score: "6",
                game_status: "Top 9th",
                game_date: "July 10, 2026",
                venue: "Test Ballpark · Seattle, WA",
            },
        )
        .unwrap();
        let html = String::from_utf8(out).unwrap();
        assert!(!html.contains("/rsjs-runtime.js"));
        assert!(html.contains("<strong>7</strong>"));
        assert!(html.contains("<strong>6</strong>"));
        assert!(html.contains("Top 9th"));
        assert!(html.contains("July 10, 2026"));
        assert!(html.contains("Test Ballpark · Seattle, WA"));
    }

    #[test]
    fn renders_complete_game_data_from_props() {
        let mut data = GAME.clone();
        data.away.name = "Custom Visitors".into();
        data.away.linescore[0] = "9".into();
        data.away_batting[0].name = "Dynamic Batter".into();
        data.away_batting[0].stats[0] = "4-4".into();
        data.away_pitching[0].name = "Dynamic Pitcher".into();
        data.plays[0].text = "Dynamic scoring play from JSON props.".into();
        data.news[0].headline = "Dynamic sports news headline".into();
        data.top_performers[0].name = "Dynamic Performer".into();
        data.game_information[0].value = "Dynamic Stadium".into();
        data.recap.headline = "Dynamic recap headline".into();

        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);
        render_into(&mut cx, &data).unwrap();
        let html = String::from_utf8(out).unwrap();

        for expected in [
            "Custom Visitors",
            "Dynamic Batter",
            "4-4",
            "Dynamic Pitcher",
            "Dynamic scoring play from JSON props.",
            "Dynamic sports news headline",
            "Dynamic Performer",
            "Dynamic Stadium",
            "Dynamic recap headline",
        ] {
            assert!(html.contains(expected), "missing dynamic value: {expected}");
        }
    }
}
