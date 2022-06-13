#[cfg(unix)]
unsafe fn unix_set_soft_limit_to_hard() {
	use libc::{getrlimit, rlimit, RLIMIT_NOFILE, setrlimit};

	let mut limits = rlimit {
		rlim_cur: 0,
		rlim_max: 0
	};
	if getrlimit(RLIMIT_NOFILE, &mut limits) != 0 {
		panic!("getrlimit didn't return with success");
	}
	limits.rlim_cur = limits.rlim_max;
	if setrlimit(RLIMIT_NOFILE, &limits) != 0 {
		panic!("setrlimit didn't return with success");
	}
}

pub(crate) fn set_soft_limit_to_hard() {
	#[cfg(unix)]
	unsafe { unix_set_soft_limit_to_hard() }
}