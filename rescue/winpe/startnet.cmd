@echo off
rem Rebootly WinPE: put into the image as Windows\System32\startnet.cmd by
rem build.ps1. Starts WinPE (drivers, network), then Rebootly from its stick.
wpeinit
call rebootly.cmd
