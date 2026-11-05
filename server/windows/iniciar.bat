@echo off
rem PokeGrid Remoto no PC: as contas rodam aqui e voce acompanha pelo navegador (localhost ou o dominio)
title PokeGrid Remoto
chcp 65001 >nul
cd /d "%~dp0"
if not exist "%~dp0pokegrid.conf" (
  echo Falta o pokegrid.conf: copie o pokegrid.conf.example para pokegrid.conf e preencha.
  pause
  exit /b 1
)
echo Abra http://localhost:8080 neste PC. Fechar esta janela desliga as contas.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0pokegrid.ps1"
pause
