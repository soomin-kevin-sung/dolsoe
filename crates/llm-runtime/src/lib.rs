pub fn expected_abi() -> (u32, u32) {
    (llm_runtime_sys::ABI_MAJOR, llm_runtime_sys::ABI_MINOR)
}

#[cfg(test)]
mod tests {
    use super::expected_abi;

    #[test]
    fn expected_abi_matches_runtime_sys_version() {
        assert_eq!(expected_abi(), (1, 0));
    }
}
