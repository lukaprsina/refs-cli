# Windows entry point for scripts/check. Plain `bash` resolves to WSL here, so call Git Bash.
& "$env:ProgramFiles\Git\bin\bash.exe" "$PSScriptRoot/check" @args
exit $LASTEXITCODE
