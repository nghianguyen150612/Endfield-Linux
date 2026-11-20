#!/usr/bin/env python3
"""Harmless Linux fixture: held ELF identity does not bind pathname helpers."""
import ctypes
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SOURCE = r'''
#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
int main(int argc, char **argv) {
    (void)argv;
    char path[4096];
    ssize_t n = readlink("/proc/self/exe", path, sizeof(path)-1);
    if (n < 0) return 2;
    path[n] = 0;
    if (argc == 1) { puts(path); return 0; }
    char *end = strrchr(path, '/');
    if (!end) return 3;
    strcpy(end+1, "helper");
    execl(path, path, (char *)NULL);
    return 4;
}
'''


class DescriptorContract(unittest.TestCase):
    def run_descriptor(self, fd, chained=False):
        read, write = os.pipe()
        pid = os.fork()
        if pid == 0:
            os.close(read)
            os.dup2(write, 1)
            os.close(write)
            argv = (ctypes.c_char_p * 3)(b'fixture', b'chain' if chained else None, None)
            env = (ctypes.c_char_p * 1)(None)
            ctypes.CDLL(None).execveat(fd, b'', argv, env, 0x1000)
            os._exit(127)
        os.close(write)
        with os.fdopen(read, 'rb') as stream:
            output = stream.read(8192)
        _, status = os.waitpid(pid, 0)
        return os.waitstatus_to_exitcode(status), output.decode().strip()

    def test_path_rename_replacement_and_unbound_helper(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            directory = root/'generation'; directory.mkdir()
            source = root/'fixture.c'; source.write_text(SOURCE)
            launcher = directory/'launcher'
            subprocess.run(['gcc','-std=c17','-Wall','-Wextra','-Werror',str(source),'-o',str(launcher)],check=True)
            fd = os.open(launcher, os.O_RDONLY | os.O_CLOEXEC)
            try:
                self.assertEqual(self.run_descriptor(fd), (0,str(launcher)))
                moved = root/'moved'; directory.rename(moved)
                self.assertEqual(self.run_descriptor(fd), (0,str(moved/'launcher')))
                directory.mkdir(); (directory/'launcher').write_bytes(b'not ELF')
                self.assertEqual(self.run_descriptor(fd), (0,str(moved/'launcher')))
                # Pathname helper is outside held-descriptor authentication.
                helper_source = root/'helper.c'
                helper_source.write_text('#include <stdio.h>\nint main(void){puts("substituted");return 0;}\n')
                subprocess.run(['gcc','-std=c17','-Wall','-Wextra','-Werror',str(helper_source),'-o',str(moved/'helper')],check=True)
                self.assertEqual(self.run_descriptor(fd, True), (0,'substituted'))
                (moved/'launcher').unlink()
                self.assertEqual(self.run_descriptor(fd), (0,str(moved/'launcher')+' (deleted)'))
            finally:
                os.close(fd)


if __name__ == '__main__':
    unittest.main()
