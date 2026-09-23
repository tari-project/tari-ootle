// The only prettier config in the repo. A per-package override cannot be limited to its own
// package: prettier loads each config once per run and a config that mutates the object it
// imports from here changes formatting for every other package too, depending on which file
// prettier reached first. So there is one config, and it applies to every TypeScript source
// file the `fmt` scripts cover.
export default {
  "printWidth": 120,
  "tabWidth": 2,
  "useTabs": false,
  "semi": true,
  "singleQuote": false,
  "quoteProps": "consistent",
  "trailingComma": "all",
  "bracketSpacing": true,
  "bracketSameLine": false,
  "arrowParens": "always",
  "endOfLine": "lf",
  "plugins": ["prettier-plugin-organize-imports"],
};
