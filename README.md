# findsource

Simple configurable tool for searching source files by extensions easily!

## Help

```txt
Usage: fs [-d,--debug] [-?,--help] [-v,--verbose] [-l,--load CFG|PATH] [-w,--whole] [-W,--Whole] [-e,--extension] [-E,--Extension]
       [-X,--Exclude] [-i,--ignore-case] [-o,--only] [-/r,--/reverse] [-a,--hidden] [-f,--full] [-inv,--invert] [ARGS]

Simple configurable tool for searching source files by extensions easily!

OPTION:
  -d,--debug              Print debug message
  -?,--help               Print help message
  -v,--verbose            Print more debug message
  -l,--load CFG|PATH      Load option setting from configuration
                          name or file
  -w,--whole              Extension category: match whole filename
  -W,--Whole              Exclude given whole filename
  -e,--extension          Extension category: match file extension
  -E,--Extension          Exclude given file extension
  -X,--Exclude            Exclude given file category
  -i,--ignore-case        Enable ignore case mode
  -o,--only               Only search given file category
  -/r,--/reverse          Disable reverse mode
  -a,--hidden             Search hidden file
  -f,--full               Display absolute path of matched file
  -inv,--invert           Invert the entrie logical to exclude the
                          given extension
ARGS:
  [PATH]+      Path need to be search

Create by araraloren <blackcatoverwall@gmail.com> v0.2.0
```

`fs` will search for the configuration file in `executable binary directory`, `working directory`,
`current directory` and two custome directories.
The first is `executable binary directory/FS_BUILD_CONFIG_DIR` which can be set at compile time,
then `FS_CONFIG_DIR` which can be set at runtime.

## Get the release 

Get [Release](https://github.com/araraloren/findsource/releases) here.

## LICENSE

MPL-2.0