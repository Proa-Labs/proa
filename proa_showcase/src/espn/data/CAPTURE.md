# ESPN capture

- Page: `https://www.espn.com/mlb/boxscore/_/gameId/401695777`
- Summary endpoint: `https://site.api.espn.com/apis/site/v2/sports/baseball/mlb/summary?event=401695777`
- Event: New York Yankees at Los Angeles Dodgers, May 31, 2025
- Captured: July 10, 2026 (America/Los_Angeles)

The public page returned an AWS WAF JavaScript challenge (`HTTP 202`, empty
body) to this environment. `summary-401695777.json` is the untouched response
from ESPN's page-data endpoint. The image files in `images/espn/` were
downloaded from the ESPN CDN URLs present in that response.
