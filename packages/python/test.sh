#!/bin/sh

set -eu

[ "$( python -c 'print("Hello")' )" = "Hello" ]

[ "$( python --version )" = "$( python3 --version )" ]
[ "$( ipython --version )" = "$( ipython3 --version )" ]
[ "$( pip --version )" = "$( pip3 --version )" ]

pydoc3 abs | grep -q 'absolute value'

python3-config --prefix | grep -q "$HOME/opt/python/"

black --version >/dev/null
pylama --version >/dev/null
