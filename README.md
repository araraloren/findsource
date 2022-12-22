# findsource

Find source file with extension easily!

## Help

```txt
Usage: fs [-d,--debug] [-?,--help] [-v,--verbose] [-l,--load CFG|PATH] [-w,--whole] [-W,--Whole] [-e,--extension] [-E,--Extension] [-X,--Exclude] [-i,--ignore-case] [-o,--only] [-/r,--/reverse] [-a,--hidden] [ARGS]

Simple configurable tool find source file with extension.

OPTION:
  -d,--debug              Print debug message
  -?,--help               Print help message
  -v,--verbose            Print more debug message
  -l,--load CFG|PATH      Load option setting from configuration name or file
  -w,--whole              Extension category: match whole filename
  -W,--Whole              Exclude given whole filename
  -e,--extension          Extension category: match file extension
  -E,--Extension          Exclude given file extension
  -X,--Exclude            Exclude given file category
  -i,--ignore-case        Enable ignore case mode
  -o,--only               Only search given file category
  -/r,--/reverse          Disable reverse mode
  -a,--hidden             Search hidden file

ARGS:
  [PATH]+      Path need to be search

Create by araraloren <blackcatoverwall@gmail.com> v0.1.1
```

`fs` will search configuration file in `executable binary directory`, `working directory`,
`current directory` and two custome directories.
First is `executable binary directory/FS_BUILD_CONFIG_DIR` which can set in compile time,
second is `FS_CONFIG_DIR` which can set in runtime.

## Get the release 

Get [Release](https://github.com/araraloren/findsource/releases) here.

## LICENSE

MPL-2.0