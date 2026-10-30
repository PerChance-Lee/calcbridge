import sys

if len(sys.argv) > 1 and sys.argv[1] in ("cli", "--cli"):
    from .cli import main as cli_main

    sys.exit(cli_main(sys.argv[2:]))

from .gui.app import main  # noqa: E402

sys.exit(main())
