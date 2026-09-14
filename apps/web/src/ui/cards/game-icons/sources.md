# Preview game icons

Source: https://github.com/yyf-0404/tsugu-bangdream-bot/tree/07d2dfeba87c1a83120cebbbd6e89de4517e45a7/backend/assets

- `limit-break.png`: `Card/limitBreakRank.png`, unchanged (72 × 72). Rank is rendered as live text over the original badge. Rank 0 / unowned does not render a badge.
- `../skill-icons/life.png`, `judge.png`, `damage.png`: matching files under `Skill/`, unchanged.
- Skill number and suffix convention follows `backend/src/components/skill.ts`; breakthrough composition follows `backend/src/components/card.ts`.
- These are game visual assets used in the local design preview; attribution and original provenance are retained here.

- `star.png` and `star_trained.png`: corresponding unchanged files under `Card/`; repeated vertically according to rarity, chosen by training state (independent of illustration switching).
