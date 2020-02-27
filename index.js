const path = require("path");
const fs = require("fs");
const walk = require("fs-walk");
const runImport = require("./main.js");

const getBz2Files = _path => {
  const filesPaths = [];
  // return new Promise( (resolve, reject) => {
  walk.walkSync(_path,  (basedir, filename, stat) => {
    if (path.extname(filename) === ".bz2") {
      filesPaths.push(path.join(basedir, filename));
    }
  });
  return filesPaths;
};

const run = (dataPath, configPathOrIdOrObject, _pgConfig, _options) => {

  let options = {
    "schema": _options.schema || 'public',
    "year": _options.year,
    "srid": _options.srid || 4326,
    "threads": _options.threads,
    "codeDep": _options.codeDep,
    "logLevel": _options.logLevel || 2,
    "dropSchema": _options.dropSchema ? true : false,
    "dropTable": _options.dropTable ? true : false,
  }

  let pgConfig = {
    "user": _pgConfig.user,
    "host": _pgConfig.host,
    "database": _pgConfig.database,
    "password": _pgConfig.password,
    "port": _pgConfig.port
  };


  if (!options.year) {
    return new Promise((resolve, reject) => {
      reject(`Il est obligatoire d'enter une année de réference`)
    });
  }


  if (!configPathOrIdOrObject) {
    configPathOrIdOrObject = 'light';
  }
  let dataConfig;
  let pathConfig;

  if (typeof configPathOrIdOrObject === 'string') {
    if (/\.json$/.test(configPathOrIdOrObject)) {
      pathConfig = configPathOrIdOrObject;
    }
    else {
      pathConfig = path.join(__dirname, ".", "config", `${configPathOrIdOrObject}.json`)
    }

    if (fs.existsSync(pathConfig)) {
      dataConfig = JSON.parse(fs.readFileSync(pathConfig, "utf8"));
    } else {
      return new Promise((resolve, reject) => {
        reject(`${configPathOrIdOrObject} "N'existe pas !"`)
      });
    }

  } else {
    dataConfig = configPathOrIdOrObject;
  }



  const files = getBz2Files(dataPath);
  console.time('time')
  return runImport(files, dataConfig, pgConfig, options)
}







module.exports = run;