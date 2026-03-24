# Python 실행

이 Windows 환경에서 `python3` 명령은 정상 동작하지 않는다 (exit code 49, `-c` 인자 무시). Python이 필요할 때는 반드시 `python`을 사용할 것.

```bash
# 잘못됨 - 동작하지 않음
python3 -c "print('hello')"

# 올바름
python -c "print('hello')"
```
