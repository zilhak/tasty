# Windows 프로세스 정리

`process.kill()`은 Windows에서 자식 프로세스를 종료하지 않는다. 프로세스 트리 전체를 종료하려면 `taskkill /F /T /PID`를 사용해야 한다. 테스트 harness의 Drop에서 이미 적용되어 있다.
