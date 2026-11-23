// The same program as `stagepack-studio`, always of the console subsystem. On Windows it is staged as `stagepack-studio.com`
// beside `stagepack-studio.exe`: typing `stagepack-studio` in cmd or PowerShell runs the `.com` (PATHEXT tries `.COM` first),
// which the shell waits for, so output, standard input and the exit status behave as for any console program.

fn main() {
    stagepack_studio::run_main();
}
