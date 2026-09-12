// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import React, { useState } from "react";

type Team = {
  abbreviation: string;
  location: string;
  name: string;
  record: string;
  logo: string;
  score: string;
  linescore: string[];
  hits: string;
  errors: string;
};

type Batter = { name: string; position: string; stats: string[] };
type Pitcher = { name: string; note: string; stats: string[] };
type Play = { inning: string; text: string; score: string; scoring: boolean };
type NewsItem = { image: string; headline: string; description: string };
type GameInfoItem = { label: string; value: string };
type Recap = { image: string; imageAlt: string; category: string; headline: string; description: string; linkLabel: string };
type TopPerformer = { logo: string; name: string; position: string; line: string; note: string };

export type EspnGamecastData = {
  league: string;
  gameStatus: string;
  gameDate: string;
  shortDate: string;
  startTime: string;
  venue: string;
  winningPitcher: string;
  losingPitcher: string;
  gameInformation: GameInfoItem[];
  recap: Recap;
  topPerformers: TopPerformer[];
  away: Team;
  home: Team;
  awayBatting: Batter[];
  homeBatting: Batter[];
  awayPitching: Pitcher[];
  homePitching: Pitcher[];
  plays: Play[];
  news: NewsItem[];
};

// Preserve the pre-brand-restoration export for downstream demo consumers.
export type SportsGamecastData = EspnGamecastData;

const away: Team = {
  abbreviation: "NYY",
  location: "New York",
  name: "Yankees",
  record: "35-22",
  logo: "/images/espn/nyy.png",
  score: "2",
  linescore: ["0", "0", "0", "1", "0", "0", "0", "1", "0"],
  hits: "7",
  errors: "0",
};
const home: Team = {
  abbreviation: "LAD",
  location: "Los Angeles",
  name: "Dodgers",
  record: "36-22",
  logo: "/images/espn/lad.png",
  score: "18",
  linescore: ["4", "6", "0", "0", "4", "0", "1", "3", "—"],
  hits: "21",
  errors: "0",
};

const nyyBatting: Batter[] = [
  ["T. Grisham", "CF", "0-3", "3", "0", "0", "0", "0", "1", "2"],
  ["A. Judge", "RF", "3-4", "4", "2", "3", "2", "2", "0", "0"],
  ["C. Bellinger", "LF", "1-4", "4", "0", "1", "0", "0", "0", "0"],
  ["P. Goldschmidt", "1B", "0-3", "3", "0", "0", "0", "0", "0", "0"],
  ["J.C. Escarra", "C", "0-1", "1", "0", "0", "0", "0", "0", "0"],
  ["J. Domínguez", "RF", "1-4", "4", "0", "1", "0", "0", "0", "3"],
  ["A. Volpe", "SS", "0-2", "2", "0", "0", "0", "0", "1", "1"],
  ["O. Peraza", "2B", "0-1", "1", "0", "0", "0", "0", "0", "0"],
  ["A. Wells", "C", "1-4", "4", "0", "1", "0", "0", "0", "2"],
  ["D. LeMahieu", "2B", "0-2", "2", "0", "0", "0", "0", "2", "1"],
  ["J. Vivas", "3B", "1-3", "3", "0", "1", "0", "0", "0", "0"],
].map(([name, position, ...stats]) => ({ name, position, stats }));

const ladBatting: Batter[] = [
  ["S. Ohtani", "DH", "2-4", "4", "2", "2", "0", "0", "0", "2"],
  ["D. Rushing", "C", "1-2", "2", "1", "1", "3", "1", "0", "0"],
  ["T. Hernández", "LF", "0-3", "3", "1", "0", "0", "0", "1", "2"],
  ["E. Hernández", "1B", "2-2", "2", "0", "2", "0", "0", "0", "0"],
  ["F. Freeman", "1B", "2-3", "3", "2", "2", "1", "0", "1", "0"],
  ["M. Rojas", "SS", "0-2", "2", "0", "0", "0", "0", "0", "0"],
  ["W. Smith", "C", "2-6", "6", "2", "2", "1", "0", "0", "1"],
  ["M. Muncy", "3B", "3-6", "6", "3", "3", "7", "2", "0", "0"],
  ["A. Pages", "CF", "1-4", "4", "1", "1", "1", "1", "2", "0"],
  ["M. Conforto", "DH", "0-2", "2", "1", "0", "1", "0", "2", "0"],
  ["T. Edman", "LF", "4-5", "5", "2", "4", "2", "0", "0", "0"],
  ["H. Kim", "SS", "4-4", "4", "3", "4", "2", "1", "1", "0"],
].map(([name, position, ...stats]) => ({ name, position, stats }));

const nyyPitching: Pitcher[] = [
  ["W. Warren", "L, 3-3", "1.1", "6", "7", "7", "4", "2", "1", "57-29", "5.19"],
  ["B. Headrick", "", "0.2", "2", "3", "3", "1", "1", "1", "20-14", "3.24"],
  ["I. Hamilton", "", "2.0", "0", "0", "0", "1", "1", "0", "30-20", "4.50"],
  ["M. Leiter Jr.", "", "0.2", "4", "4", "4", "0", "0", "1", "19-13", "3.70"],
  [
    "Y. De los Santos",
    "",
    "0.2",
    "3",
    "0",
    "0",
    "1",
    "0",
    "0",
    "23-14",
    "3.00",
  ],
  ["D. Williams", "", "0.2", "0", "0", "0", "0", "0", "0", "6-3", "6.53"],
  ["L. Weaver", "", "1.0", "1", "1", "1", "0", "1", "1", "14-9", "1.05"],
  ["P. Reyes", "", "1.0", "5", "3", "3", "0", "0", "1", "18-12", "27.00"],
].map(([name, note, ...stats]) => ({ name, note, stats }));

const ladPitching: Pitcher[] = [
  ["L. Knack", "W, 3-2", "6.0", "5", "1", "1", "3", "6", "1", "102-63", "4.58"],
  ["A. Banda", "", "1.0", "0", "0", "0", "1", "2", "0", "19-11", "3.86"],
  ["C. Stratton", "", "1.0", "1", "1", "1", "0", "1", "1", "18-13", "8.00"],
  ["E. Hernández", "", "1.0", "1", "0", "0", "0", "0", "0", "7-5", "0.00"],
].map(([name, note, ...stats]) => ({ name, note, stats }));

const plays: Play[] = [
  [
    "1st Inning",
    "Smith singled to right, Ohtani scored, Smith to second, Freeman to third.",
    "NYY 0, LAD 1",
    true,
  ],
  [
    "1st Inning",
    "Muncy singled to center, Freeman scored, Smith to third.",
    "NYY 0, LAD 2",
    true,
  ],
  [
    "1st Inning",
    "Conforto hit sacrifice fly to right, Smith scored, Muncy to third.",
    "NYY 0, LAD 3",
    true,
  ],
  [
    "1st Inning",
    "Edman doubled to left, Muncy scored, Pages to third.",
    "NYY 0, LAD 4",
    true,
  ],
  [
    "2nd Inning",
    "Muncy homered to center (400 feet), T. Hernández scored and Smith scored.",
    "NYY 0, LAD 7",
    true,
  ],
  [
    "2nd Inning",
    "Edman doubled to left, Conforto scored.",
    "NYY 0, LAD 8",
    true,
  ],
  [
    "2nd Inning",
    "Kim homered to right center (412 feet), Edman scored.",
    "NYY 0, LAD 10",
    true,
  ],
  ["4th Inning", "Judge homered to center (446 feet).", "NYY 1, LAD 10", true],
  [
    "5th Inning",
    "Muncy homered to right, Freeman and Smith scored.",
    "NYY 1, LAD 14",
    true,
  ],
  [
    "7th Inning",
    "Pages homered to left center (390 feet).",
    "NYY 1, LAD 15",
    true,
  ],
  [
    "8th Inning",
    "Rushing homered to left, Edman and Kim scored.",
    "NYY 1, LAD 18",
    true,
  ],
  [
    "9th Inning",
    "Judge homered to left center (395 feet).",
    "NYY 2, LAD 18",
    true,
  ],
  ["9th Inning", "Peraza grounded out to third.", "NYY 2, LAD 18", false],
  [
    "9th Inning",
    "Wells grounded out to shortstop, Domínguez to third.",
    "NYY 2, LAD 18",
    false,
  ],
  ["9th Inning", "LeMahieu flied out to center.", "Final", false],
].map(([inning, text, score, scoring]) => ({
  inning: String(inning),
  text: String(text),
  score: String(score),
  scoring: Boolean(scoring),
}));

const news: NewsItem[] = [
  {
    image: "/images/espn/wetherholt.jpg",
    headline: "Rookie JJ Wetherholt, Cardinals agree to 8-year deal, sources say",
    description: "Infielder JJ Wetherholt and the Cardinals are in agreement on an eight-year extension, sources told ESPN.",
  },
  {
    image: "/images/espn/ohtani-news.jpg",
    headline: "Shohei Ohtani dealing with knee irritation, to miss All-Star Game",
    description: "Ohtani was scratched from his scheduled pitching start due to continued irritation in his left knee.",
  },
  {
    image: "/images/espn/nance-news.jpg",
    headline: "Twins acquire reliever Tommy Nance from Blue Jays",
    description: "Minnesota added the right-handed reliever in a trade with Toronto.",
  },
];

export const DEFAULT_ESPN_GAME_DATA: EspnGamecastData = {
  league: "MLB",
  gameStatus: "Final",
  gameDate: "May 31, 2025",
  shortDate: "May 31",
  startTime: "7:15 PM",
  venue: "Dodger Stadium · Los Angeles, CA",
  winningPitcher: "L. Knack (3-2)",
  losingPitcher: "W. Warren (3-3)",
  gameInformation: [
    { label: "Venue", value: "Dodger Stadium" },
    { label: "Location", value: "Los Angeles, California" },
    { label: "Attendance", value: "51,746" },
    { label: "Game Time", value: "3:04" },
  ],
  recap: {
    image: "/images/espn/muncy-recap.jpg",
    imageAlt: "Max Muncy celebrates a home run",
    category: "MLB",
    headline: "Muncy powers Dodgers with 7 RBIs in 18-2 rout of Yankees",
    description: "Max Muncy hit two three-run homers and tied his career high with seven RBI, helping Los Angeles post its most runs ever against New York.",
    linkLabel: "Watch highlights",
  },
  topPerformers: [
    { logo: "/images/espn/lad.png", name: "Max Muncy", position: "LAD · 3B", line: "3-6", note: "2 HR, 7 RBI" },
    { logo: "/images/espn/nyy.png", name: "Aaron Judge", position: "NYY · RF", line: "3-4", note: "2 HR, 2 RBI" },
  ],
  away,
  home,
  awayBatting: nyyBatting,
  homeBatting: ladBatting,
  awayPitching: nyyPitching,
  homePitching: ladPitching,
  plays,
  news,
};

export const DEFAULT_SPORTS_GAME_DATA = DEFAULT_ESPN_GAME_DATA;

function TopChrome({ data }: { data: EspnGamecastData }) {
  return (
    <>
      <header className="espn-header">
        <div className="espn-header__inner">
          <button className="espn-menu" type="button" aria-label="Open menu">
            <i />
            <i />
            <i />
          </button>
          <a className="espn-logo" href="#" aria-label="ESPN">
            ESPN
          </a>
          <nav className="espn-global" aria-label="ESPN sections">
            <a href="#">NFL</a>
            <a href="#">NBA</a>
            <a href="#">MLB</a>
            <a href="#">NHL</a>
            <a href="#">Soccer</a>
            <a href="#">
              More<span>⌄</span>
            </a>
          </nav>
          <div className="espn-account">
            <button type="button" aria-label="Search">
              ⌕
            </button>
            <button type="button">Log In</button>
          </div>
        </div>
      </header>
      <div className="espn-leaguebar">
        <div className="espn-leaguebar__inner">
          <strong>{data.league}</strong>
          <a href="#">Home</a>
          <a href="#">Scores</a>
          <a href="#">Schedule</a>
          <a href="#">Standings</a>
          <a href="#">Stats</a>
          <a href="#">Teams</a>
          <a href="#">Transactions</a>
        </div>
      </div>
      <section className="score-strip" aria-label="Other MLB scores">
        <div className="score-strip__inner">
          <button type="button" aria-label="Previous scores">
            ‹
          </button>
          {[
            ["BOS", "4", "ATL", "2"],
            ["CHC", "1", "CIN", "6"],
            [data.away.abbreviation, data.away.score, data.home.abbreviation, data.home.score],
            ["SF", "3", "MIA", "4"],
            ["MIN", "2", "SEA", "5"],
          ].map((g, i) => (
            <div
              className={`score-tile${i === 2 ? " score-tile--active" : ""}`}
              key={g[0]}
            >
              <span>FINAL</span>
              <b>{g[0]}</b>
              <strong>{g[1]}</strong>
              <b>{g[2]}</b>
              <strong>{g[3]}</strong>
            </div>
          ))}
          <button type="button" aria-label="Next scores">
            ›
          </button>
        </div>
      </section>
    </>
  );
}

function LineScore({ away, home }: { away: Team; home: Team }) {
  return (
    <div className="line-score-wrap">
      <table className="line-score">
        <thead>
          <tr>
            <th />
            {[1, 2, 3, 4, 5, 6, 7, 8, 9].map((n) => (
              <th key={n}>{n}</th>
            ))}
            <th>R</th>
            <th>H</th>
            <th>E</th>
          </tr>
        </thead>
        <tbody>
          {[away, home].map((team) => (
            <tr key={team.abbreviation}>
              <th>{team.abbreviation}</th>
              {team.linescore.map((n, i) => (
                <td key={i}>{n}</td>
              ))}
              <td className="total">{team.score}</td>
              <td>{team.hits}</td>
              <td>{team.errors}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function Scoreboard({ data }: { data: EspnGamecastData }) {
  const { away, home } = data;
  return (
    <section className="matchup-card" data-espn-scoreboard>
      <div className="matchup-card__meta">
        <span>{data.league}</span>
        <time>{data.gameDate}</time>
        <span>{data.venue}</span>
      </div>
      <div className="matchup-card__body">
        <div className="team-block team-block--away">
          <img src={away.logo} alt="New York Yankees" />
          <div>
            <p>{away.location}</p>
            <h1>{away.name}</h1>
            <span>{away.record}</span>
          </div>
          <strong>{away.score}</strong>
        </div>
        <div className="game-status">
          <b>{data.gameStatus}</b>
          <span>
            R<i>H</i>
            <i>E</i>
          </span>
          <p>
            {data.shortDate}
            <br />
            {data.startTime}
          </p>
        </div>
        <div className="team-block team-block--home">
          <strong>{home.score}</strong>
          <div>
            <p>{home.location}</p>
            <h1>{home.name}</h1>
            <span>{home.record}</span>
          </div>
          <img src={home.logo} alt="Los Angeles Dodgers" />
        </div>
      </div>
      <LineScore away={away} home={home} />
      <div className="matchup-card__footer">
        <div>
          <b>W: </b>{data.winningPitcher}
        </div>
        <div>
          <b>L: </b>{data.losingPitcher}
        </div>
        <a href="#recap">Recap</a>
      </div>
    </section>
  );
}

function TeamBox({
  team,
  batting,
  pitching,
}: {
  team: Team;
  batting: Batter[];
  pitching: Pitcher[];
}) {
  const lad = team.abbreviation === "LAD";
  return (
    <article className="stat-card">
      <header className="stat-card__team">
        <img src={team.logo} alt="" />
        <div>
          <b>
            {team.location} {team.name}
          </b>
          <span>{team.record}</span>
        </div>
        <strong>{team.score}</strong>
      </header>
      <div className="table-scroll">
        <table className="player-table batting-table">
          <caption>{team.name} Batting</caption>
          <thead>
            <tr>
              {["Hitters", "H-AB", "AB", "R", "H", "RBI", "HR", "BB", "K"].map(
                (x) => (
                  <th key={x}>{x}</th>
                ),
              )}
            </tr>
          </thead>
          <tbody>
            {batting.map((p) => (
              <tr key={`${p.name}-${p.position}`}>
                <th>
                  <a href="#player">{p.name}</a>
                  <span>{p.position}</span>
                </th>
                {p.stats.map((x, i) => (
                  <td key={i}>{x}</td>
                ))}
              </tr>
            ))}
          </tbody>
          <tfoot>
            <tr>
              <th>Totals</th>
              <td />
              <td>38</td>
              <td>{team.score}</td>
              <td>{team.hits}</td>
              <td>{lad ? "18" : "2"}</td>
              <td>{lad ? "5" : "2"}</td>
              <td>6</td>
              <td>10</td>
            </tr>
          </tfoot>
        </table>
      </div>
      <div className="stat-notes">
        <b>Batting</b>
        <p>
          2B: {lad ? "Edman 2 (15), Freeman (18), Smith (9)" : "Judge (13)"}
        </p>
        <p>
          HR:{" "}
          {lad
            ? "Muncy 2 (7), Kim (3), Pages (11), Rushing (2)"
            : "Judge 2 (21)"}
        </p>
      </div>
      <div className="table-scroll">
        <table className="player-table pitching-table">
          <caption>{team.name} Pitching</caption>
          <thead>
            <tr>
              {[
                "Pitchers",
                "IP",
                "H",
                "R",
                "ER",
                "BB",
                "K",
                "HR",
                "PC-ST",
                "ERA",
              ].map((x) => (
                <th key={x}>{x}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {pitching.map((p) => (
              <tr key={p.name}>
                <th>
                  <a href="#player">{p.name}</a>
                  <span>{p.note}</span>
                </th>
                {p.stats.map((x, i) => (
                  <td key={i}>{x}</td>
                ))}
              </tr>
            ))}
          </tbody>
          <tfoot>
            <tr>
              <th>Totals</th>
              <td>8.0</td>
              <td>{lad ? "7" : "21"}</td>
              <td>{lad ? "2" : "18"}</td>
              <td>{lad ? "2" : "18"}</td>
              <td>6</td>
              <td>10</td>
              <td>{lad ? "2" : "5"}</td>
              <td>187-114</td>
              <td />
            </tr>
          </tfoot>
        </table>
      </div>
    </article>
  );
}

function PlayList({ plays, scoringOnly = false }: { plays: Play[]; scoringOnly?: boolean }) {
  let previous = "";
  return (
    <>
      {plays
        .filter((p) => !scoringOnly || p.scoring)
        .map((play, i) => {
          const heading = play.inning !== previous;
          previous = play.inning;
          return (
            <div key={`${play.inning}-${i}`}>
              {heading && <h4 className="inning-heading">{play.inning}</h4>}
              <div className="play-row">
                <span className="play-icon">{play.scoring ? "⚾" : "•"}</span>
                <p>{play.text}</p>
                <strong>{play.score}</strong>
              </div>
            </div>
          );
        })}
    </>
  );
}

function BoxScorePanel({ data }: { data: EspnGamecastData }) {
  return (
    <>
      <div className="section-title">
        <h2>Box Score</h2>
        <span>{data.gameStatus}</span>
      </div>
      <TeamBox team={data.away} batting={data.awayBatting} pitching={data.awayPitching} />
      <TeamBox team={data.home} batting={data.homeBatting} pitching={data.homePitching} />
      <article className="panel-card game-info">
        <header>
          <h3>Game Information</h3>
        </header>
        <img src="/images/espn/dodger-stadium.jpg" alt="Dodger Stadium" />
        <dl>
          {data.gameInformation.map((item) => (
            <div key={item.label}>
              <dt>{item.label}</dt>
              <dd>{item.value}</dd>
            </div>
          ))}
        </dl>
      </article>
    </>
  );
}

function GamecastPanel({ data }: { data: EspnGamecastData }) {
  return (
    <>
      <div className="section-title">
        <h2>Gamecast</h2>
        <span>{data.gameStatus}</span>
      </div>
      <article className="recap-card" id="recap">
        <div className="recap-card__image">
          <img
            src={data.recap.image}
            alt={data.recap.imageAlt}
          />
          <i>▶</i>
        </div>
        <div className="recap-card__copy">
          <span>{data.recap.category}</span>
          <h3>{data.recap.headline}</h3>
          <p>{data.recap.description}</p>
          <a href="#">{data.recap.linkLabel}</a>
        </div>
      </article>
      <article className="panel-card win-card">
        <header>
          <h3>Win Probability</h3>
          <button type="button">Game Flow</button>
        </header>
        <div className="win-legend">
          <span>
            <i className="nyy-dot" />
            NYY 0%
          </span>
          <span>
            <i className="lad-dot" />
            LAD 100%
          </span>
        </div>
        <svg
          viewBox="0 0 700 230"
          role="img"
          aria-label="Dodgers win probability rose from 52 to 100 percent"
        >
          <line x1="32" y1="18" x2="32" y2="194" />
          <line x1="32" y1="194" x2="682" y2="194" />
          <line x1="32" y1="106" x2="682" y2="106" className="mid" />
          <polyline points="32,103 74,40 108,33 146,25 184,27 226,20 270,19 312,17 358,15 402,14 452,13 500,12 550,11 608,10 682,9" />
          {[1, 2, 3, 4, 5, 6, 7, 8, 9].map((n, i) => (
            <text key={n} x={i === 8 ? 664 : 25 + i * 77} y="213">
              {n}
            </text>
          ))}
        </svg>
      </article>
      <article className="panel-card scoring-card">
        <header>
          <h3>Scoring Summary</h3>
        </header>
        <PlayList plays={data.plays} scoringOnly />
      </article>
    </>
  );
}

function Sidebar({ data }: { data: EspnGamecastData }) {
  return (
    <aside className="espn-sidebar">
      <div className="espn-ad espn-ad--box">
        <span>ADVERTISEMENT</span>
      </div>
      <article className="panel-card top-performers">
        <header>
          <h3>Top Performers</h3>
        </header>
        {data.topPerformers.map((performer) => (
          <div key={performer.name}>
            <img src={performer.logo} alt="" />
            <p>
              <b>{performer.name}</b>
              <span>{performer.position}</span>
            </p>
            <strong>
              {performer.line}
              <small>{performer.note}</small>
            </strong>
          </div>
        ))}
      </article>
      <article className="panel-card news-card">
        <header>
          <h3>MLB News</h3>
        </header>
        {data.news.map((item) => (
          <a className="news-row" href="#" key={item.headline}>
            <img src={item.image} alt="" />
            <span>
              <b>{item.headline}</b>
              <small>{item.description}</small>
            </span>
          </a>
        ))}
        <a className="more-news" href="#">
          All MLB News →
        </a>
      </article>
    </aside>
  );
}

function GameTabs({ data }: { data: EspnGamecastData }) {
  const [active, setActive] = useState<"gamecast" | "box" | "plays">("box");
  return (
    <div className="game-tabs-shell">
      <nav className="game-tabs" aria-label="Game details">
        <button
          type="button"
          aria-selected={active === "gamecast"}
          onClick={() => setActive("gamecast")}
        >
          Gamecast
        </button>
        <button
          type="button"
          aria-selected={active === "box"}
          onClick={() => setActive("box")}
        >
          Box Score
        </button>
        <button
          type="button"
          aria-selected={active === "plays"}
          onClick={() => setActive("plays")}
        >
          Play-by-Play
        </button>
        <button type="button" aria-selected={false}>
          Videos
        </button>
      </nav>
      <div className="espn-layout">
        <div className="espn-primary">
          <section hidden={active !== "gamecast"} data-panel="gamecast">
            <GamecastPanel data={data} />
          </section>
          <section hidden={active !== "box"} data-panel="box-score">
            <BoxScorePanel data={data} />
          </section>
          <section hidden={active !== "plays"} data-panel="play-by-play">
            <div className="section-title">
              <h2>Play-by-Play</h2>
              <span>{data.gameStatus}</span>
            </div>
            <article className="panel-card plays-card">
              <header>
                <h3>All Plays</h3>
                <select aria-label="Select inning">
                  <option>All Innings</option>
                  <option>9th Inning</option>
                </select>
              </header>
              <PlayList plays={data.plays} />
            </article>
          </section>
        </div>
        <Sidebar data={data} />
      </div>
    </div>
  );
}

function Footer() {
  return (
    <footer className="espn-footer">
      <div>
        <b className="espn-logo">ESPN</b>
        <nav>
          <a href="#">Terms of Use</a>
          <a href="#">Privacy Policy</a>
          <a href="#">Your US State Privacy Rights</a>
          <a href="#">Children&apos;s Online Privacy Policy</a>
        </nav>
        <p>© 2026 ESPN Enterprises, Inc. All rights reserved.</p>
      </div>
    </footer>
  );
}

export function EspnGamecastDocument({
  css = "",
  data = DEFAULT_ESPN_GAME_DATA,
}: { css?: string; data?: EspnGamecastData }) {
  const seedJson = JSON.stringify(data).replace(/</g, "\\u003c");
  return (
    <html lang="en">
      <head>
        <meta charSet="utf-8" />
        <meta name="viewport" content="width=device-width, initial-scale=1" />
        <meta name="robots" content="noindex, nofollow" />
        <title>{`${data.away.name} ${data.away.score}-${data.home.score} ${data.home.name} (${data.gameDate}) Box Score - ESPN`}</title>
        <style
          id="espn-gamecast-css"
          dangerouslySetInnerHTML={{ __html: css }}
        />
      </head>
      <body data-espn-gamecast="">
        <TopChrome data={data} />
        <div className="espn-ad espn-ad--top">
          <span>ADVERTISEMENT</span>
        </div>
        <main className="espn-page">
          <Scoreboard data={data} />
          <GameTabs data={data} />
        </main>
        <Footer />
        <script id="espn-gamecast-props" type="application/json" dangerouslySetInnerHTML={{ __html: seedJson }} />
        <script type="module" src="/espn-client.bundle.js" />
      </body>
    </html>
  );
}

export const SportsGamecastDocument = EspnGamecastDocument;
