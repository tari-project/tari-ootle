import base from "../../../.prettierrc.mjs";

base.plugins = base.plugins || [ ];
base.plugins.push("prettier-plugin-organize-imports");

export default base;
