use config_utils::ListConfig;
use merge::Merge;

pub fn default_true() -> bool {
    true
}

pub fn default_false() -> bool {
    false
}

pub fn list_config_default<T: Merge>() -> ListConfig<T> {
    ListConfig::append(vec![])
}
