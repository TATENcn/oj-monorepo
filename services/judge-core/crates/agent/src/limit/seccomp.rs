use libseccomp::{ScmpAction, ScmpFilterContext, error::SeccompError};
use shared::models::Language;

fn cpp_seccomp_filter() -> Result<ScmpFilterContext, SeccompError> {
    let filter = ScmpFilterContext::new(ScmpAction::Errno(libc::EPERM))?;

    todo!("add seccomp rules");

    Ok(filter)
}

pub fn seccomp_filter(language: Language) -> Result<ScmpFilterContext, SeccompError> {
    match language {
        Language::Cpp => cpp_seccomp_filter(),
    }
}
