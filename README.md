# findsource

Find source file with extension easily!

## Help

```txt
usage: fs [-l,--load] [-d,--debug] [-?,--help] [-w,--whole] [-W,--Whole] [-e,--extension] [-E,--Extension] [-i,--ignore-case] [-x,--exclude] [-o,--only] [-/r,--/reverse] [-a,--hidden] **ARGS**

Simple configurable tool find source file with extension.

POS:
  path@*      [file or directory]+

OPT:
  -l,--load             a      Load option setting from configuration
  -d,--debug            b      Print debug message
  -?,--help             b      Print help message
  -w,--whole            a      Extension category: match whole filename
  -W,--Whole            a      Exclude given whole filename
  -e,--extension        a      Extension category: match file extension
  -E,--Extension        a      Exclude given file extension
  -i,--ignore-case      b      Enable ignore case mode
  -x,--exclude          a      Exclude given file category
  -o,--only             s      Only search given file category
  -/r,--/reverse        b      Disable reverse mode
  -a,--hidden           b      Search hidden file

Create by araraloren <blackcatoverwall@gmail.com> v0.0.4
```

## LICENSE

MPL-2.0