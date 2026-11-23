// The same program as `stagepack-studio`, always of the console subsystem. On Windows it is staged as `stagepack-studio.com`
// beside `stagepack-studio.exe`. Wherever the shell finds the program by its bare name — `.\stagepack-studio` in its
// folder, or the folder on PATH — it runs the `.com` (PATHEXT tries `.COM` first), which the shell waits for, so output,
// standard input and the exit status behave as for any console program. The installer doesn't add the folder to PATH, so
// an assistant's MCP settings name the `.com` by its full path.

fn main() {
    stagepack_studio::run_main();
}
