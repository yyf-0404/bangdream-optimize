@{
    # An alias from ~/.ssh/config, or an IPv4 address / hostname.
    SshHost = ''

    # Empty values inherit ~/.ssh/config (or OpenSSH defaults).
    SshUser = ''
    SshPort = 0
    IdentityFile = '' # Example: '~/.ssh/id_ed25519'

    # The Linux filesystem directory in Nginx's /downloads/ alias, NOT a URL.
    # The SSH user must have write access. Use an absolute path without spaces.
    RemoteDirectory = '/var/www/bangdream-optimize/downloads'

    Target = 'x86_64-pc-windows-msvc'
    BuildJobs = 1
}
