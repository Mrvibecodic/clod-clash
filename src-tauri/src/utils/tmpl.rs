//! Some config file template

/// template for new a profile item
pub const ITEM_LOCAL: &str = "# Local profile

proxies: []

proxy-groups: []

rules: []
";

pub const ITEM_MERGE_EMPTY: &str = "# Merge: keys here override the subscription config

";

/// enhanced profile
pub const ITEM_SCRIPT: &str = "// Define main function (script entry)

function main(config, profileName) {
  return config;
}
";

/// enhanced profile
pub const ITEM_RULES: &str = "# Rules: prepend, append or delete rules of the subscription

prepend: []

append: []

delete: []
";

/// enhanced profile
pub const ITEM_PROXIES: &str = "# Proxies: prepend, append or delete proxies of the subscription

prepend: []

append: []

delete: []
";

/// enhanced profile
pub const ITEM_GROUPS: &str = "# Proxy groups: prepend, append or delete groups of the subscription

prepend: []

append: []

delete: []
";
