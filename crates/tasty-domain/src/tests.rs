//! 도메인 모델·codec·evolve·replay와 executor 시험. 저장이 필요한 시험은 실제 SQLite journal을 쓴다.

mod common;

mod codec;
mod evolve;
mod replay;
