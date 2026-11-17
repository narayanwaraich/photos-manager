/// Policy governing case-sensitivity when deriving `path_key` (spec §5.1, D26).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PathKeyPolicy {
    pub case_sensitive: bool,
}
