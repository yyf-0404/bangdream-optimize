# 确认稿页面样式与模板

这里是五个页面和侧栏的确认稿 HTML 模板与 CSS。它们最初由 `scripts/design/import-design-markup.py` 和
`scripts/design/sync-approved-design.py` 从设计工作区生成（来源文件和哈希见 `markup-sources.json`），
自 2026-11 起改为在代码中直接维护。

- 这些 CSS 按页面容器限定作用范围（`#aurora-soft-study`、`#card-library-unified`、`#player-library`、
  `#result-design`、`#archive-design`），只服务现有页面。新界面不要往这里加样式，改用
  `../tokens.css` 的设计变量和 `../components.css` 的基础组件。
- 这些样式使用共用设计变量。修改时按以下步骤检查视觉差异。
  1. `node scripts/design/ui-snapshot.mjs capture <开发地址> tmp/ui-before`
  2. 修改页面 CSS，使用 `tokens.css` 中对应的设计变量
  3. `node scripts/design/ui-snapshot.mjs capture <开发地址> tmp/ui-after`
  4. `node scripts/design/ui-snapshot.mjs compare tmp/ui-before tmp/ui-after`，确认差异都是有意为之
  5. 运行 `node --test apps/web/test/design-system.test.js` 验证样式引用
- 重新运行生成脚本需要显式传入 `--overwrite-maintained`，会覆盖这里的所有手工修改。
