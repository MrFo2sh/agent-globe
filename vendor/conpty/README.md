# Microsoft ConPTY (bundled)

`conpty.dll` and `OpenConsole.exe` (x64) from Microsoft's official NuGet package
`Microsoft.Windows.Console.ConPTY` 1.25.260930003, MIT licensed
(https://licenses.nuget.org/MIT), both Authenticode-signed by Microsoft Corporation.

The release step copies them beside `agent-globe.exe`; the terminal core (alacritty_terminal)
loads `conpty.dll` from there instead of the console host built into Windows. Measured on
2026-10-29: a line reaches the screen in ~17 ms instead of ~33 ms, and Claude Code's redraws stop
lagging. Update by fetching a newer package version and replacing both files together.
