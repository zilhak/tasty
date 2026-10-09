# pre-commit 의 플러그인 버전 검사(P.1)를 이 작업 트리에서 미룰지 정한다.
# 모드와 병합 단계의 버전 증가 절차: docs/dev-guide/git-hooks.md#lane-작업-트리의-p1-보류.
# shellcheck shell=bash

# stdout 에 deferred 나 enforced 한 단어를 쓴다. 설정을 받아들이지 않은 이유는 stderr 로 알린다.
# 미루는 조건은 셋 다다: 값이 deferred, 범위가 worktree(config.worktree), 연결된 작업 트리(주 작업 트리 아님).
# 공유 설정이나 주 작업 트리에 적힌 값은 모든 작업 트리 또는 메인 저장소의 검사를 끄게 되므로 거부한다.
plugin_bump_mode() {
    local line scope value git_dir common_dir
    if ! line=$(git config --show-scope --get tasty.pluginBump 2>/dev/null); then
        echo enforced
        return 0
    fi
    scope=${line%%$'\t'*}
    value=${line#*$'\t'}
    if [ "$value" != deferred ]; then
        printf '[P.1] tasty.pluginBump=%s 는 알 수 없는 값이다. 버전 검사를 그대로 한다.\n' "$value" >&2
        echo enforced
        return 0
    fi
    if [ "$scope" != worktree ]; then
        printf '[P.1] tasty.pluginBump=deferred 가 %s 범위에 있다. worktree 범위(git config --worktree)에 둔 값만 따른다.\n' "$scope" >&2
        echo enforced
        return 0
    fi
    git_dir=$(git rev-parse --path-format=absolute --git-dir 2>/dev/null) || git_dir=""
    common_dir=$(git rev-parse --path-format=absolute --git-common-dir 2>/dev/null) || common_dir=""
    if [ -z "$git_dir" ] || [ "$git_dir" = "$common_dir" ]; then
        printf '[P.1] tasty.pluginBump=deferred 는 연결된 작업 트리에서만 따른다. 주 작업 트리는 버전 검사를 그대로 한다.\n' >&2
        echo enforced
        return 0
    fi
    echo deferred
}
