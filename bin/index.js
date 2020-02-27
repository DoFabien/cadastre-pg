#!/usr/bin/env node
const arg = require('arg');
const cadastreToPostgis = require('../index.js');



const args = arg({
    '--path': String,
    '--year': Number,
    '--srid':    Number,
    '--config': String,
    '--threads' : Number,
    '--dep': String,

    '--logLevel': Number,
    '--dropSchema' : Boolean,
    '--dropTable' : Boolean,

    '--host':    String,
    '--database': String,
    '--user': String,
    '--password': String,
    '--port':    Number,
    '--schema': String,
    
    // Aliases
    '-h': '--host',
    '-p': '--path',
    '-y': '--year',
    '-s': '--schema'     
});



let options = {
    "schema": args['--schema'] || 'public',
    "year": args['--year'],
    "srid": args['--srid'] || 4326,
    "threads" : args['--threads'] ,
    "codeDep" : args['--dep'],
    "logLevel": args['--logLevel'] || 1,
    "dropSchema": args['--dropSchema'] ? true : false,
    "dropTable": args['--dropTable'] ? true : false,
}

let  pgConfig = {
  "user": args['--user'] ,
  "host": args['--host'],
  "database":  args['--database'],
  "password": args['--password'],
  "port": args['--port']
};

const configIdOrPath = args['--config']
const folderPath = args['--path'];

console.time('process')
cadastreToPostgis(folderPath, configIdOrPath, pgConfig , options)
.then( t => { 
    console.timeEnd('process')
})
.catch(error => {
  console.log(error);
})

// cadastre-to-postgis --path "/home/fabien/Téléchargements/dep38/38003"  --user fabien --srid 4326  --schema "cadastre" -y 2019 --logLevel 2 --dropSchema --password "fabien"
// cadastre-to-postgis --path "/home/fabien/Téléchargements/dep38/38003" --port 5432 -h "localhost" --user fabien --srid 2154 --config "bati" --schema "cadastre" -y 2019 --dep 38 --logLevel 2 --dropSchema --dropTable --password "fabien"