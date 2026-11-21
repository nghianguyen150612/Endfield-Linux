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


CHAIN_SOURCE = r'''
#define _GNU_SOURCE
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#ifndef ROLE
#define ROLE 1
#endif
int main(int argc, char **argv) {
    (void)argc; (void)argv;
    char self[4096]; ssize_t n=readlink("/proc/self/exe",self,sizeof(self)-1);
    if(n<0)return 2;
    self[n]=0;
#if ROLE == 2
    if(argc!=2)return 3;
    int fd=open(argv[1],O_RDONLY|O_NOFOLLOW);char magic[4];
    if(fd<0||read(fd,magic,4)!=4||memcmp(magic,"\177ELF",4))return 4;
    close(fd);puts("preloader-opened-launcher");fflush(stdout);
    execl(argv[1],argv[1],"continued",(char*)NULL);return 5;
#elif ROLE == 3
    puts("trusted-helper");return 0;
#elif ROLE == 4
    puts("substituted-helper");return 0;
#else
    char next[4096];strcpy(next,self);char *end=strrchr(next,'/');
    if(!end)return 6;
    if(argc==2&&!strcmp(argv[1],"continued")) {
        puts("launcher-continued");fflush(stdout);strcpy(end+1,"helper");
        execl(next,next,(char*)NULL);return 7;
    }
    puts("launcher-initial");fflush(stdout);strcpy(end+1,"preloader");
    execl(next,next,self,(char*)NULL);return 8;
#endif
}
'''


class NativeChainTests(DescriptorContract):
    def test_preloader_open_chain_and_same_uid_mutation(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);source=root/'chain.c';source.write_text(CHAIN_SOURCE)
            for role,name in [(1,'launcher'),(2,'preloader'),(3,'helper'),(4,'substitute')]:
                subprocess.run(['gcc','-std=c17','-Wall','-Wextra','-Werror','-DROLE='+str(role),str(source),'-o',str(root/name)],check=True)
            fd=os.open(root/'launcher',os.O_RDONLY|os.O_CLOEXEC)
            try:
                status,text=self.run_descriptor(fd)
                self.assertEqual(status,0)
                self.assertEqual(text.splitlines(),['launcher-initial','preloader-opened-launcher','launcher-continued','trusted-helper'])
                (root/'helper').unlink();(root/'helper').symlink_to('substitute')
                self.assertIn('substituted-helper',self.run_descriptor(fd)[1])
                # An open descriptor and owner-controlled read-only mode do not freeze bytes.
                image=(root/'launcher').read_bytes();marker=b'launcher-initial'
                (root/'launcher').chmod(0o555);(root/'launcher').chmod(0o755)
                (root/'launcher').write_bytes(image.replace(marker,b'X'*len(marker)))
                self.assertIn('X'*len(marker),self.run_descriptor(fd)[1])
            finally:os.close(fd)

    def test_interrupted_native_fixture_is_reaped(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);source=root/'pause.c'
            source.write_text('#include <unistd.h>\nint main(void){write(1,"ready\\n",6);pause();return 0;}\n')
            executable=root/'pause'
            subprocess.run(['gcc','-std=c17','-Wall','-Wextra','-Werror',str(source),'-o',str(executable)],check=True)
            with subprocess.Popen([str(executable)],stdout=subprocess.PIPE) as child:
                self.assertEqual(child.stdout.readline(),b'ready\n')
                child.kill();child.wait(timeout=3)
                self.assertLess(child.returncode,0)


if __name__ == '__main__':
    unittest.main()
