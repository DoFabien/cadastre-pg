const cluster = require("cluster");
const path = require('path');
const toGeojson = require("./toGeojson.js")
const prepareData = require('./prepareData')
const db = require("./db.js");
const getDep = require("./getDep.js");

const Flatbush = require('flatbush');

let depFeatures = undefined;
let flatbushIndex = undefined;


const runImport = async (files, config,pgConfig, options) => {

   const schema = options.schema || 'public';
   const epsg = options.srid || 4326;
   const logLevel = options.logLevel || 0;
   const year = options.year
   let numWorkers = options.threads;
   const dropSchema = options.dropSchema;
   const dropTable = options.dropTable;
   let userDep = options.codeDep;
   let withRelations = false;
   for (let id in config.tableConfig){
     if (config.tableConfig[id].type == 'relation' ){
      withRelations = true;
      break;
     }
   }
   const osCPU = require("os").cpus().length
    if (!numWorkers) {
      numWorkers = osCPU;
    }
    if (numWorkers > osCPU) {
      numWorkers = osCPU;
    }
  
    return new Promise( async (resolve, reject) => {
      let workers = [];
      if (cluster.isMaster) {
        // console.log("Master cluster setting up " + numWorkers + " workers...");

        const client = await db.dbConnect(pgConfig);
        await db.generateTable(client, config, schema, epsg, dropSchema, dropTable);
  
        for (var i = 0; i < numWorkers; i++) {
          cluster.fork();
        }
  
        cluster.on("online", worker => {
          workers = [...workers, worker.process.pid];
          // console.log("Worker " + worker.process.pid + " is online");
          worker.send({ file: files[0] });

          files.splice(0, 1);
          
  
          worker.on("exit", async message => {
            workers = workers.filter(id => id !== worker.process.pid);
            if (workers.length == 0) {
              resolve();
            }
          });
  
          worker.on("error", message => {
              reject(message);
            });
  
          worker.on("message", message => {
            // console.log(message.file);
            if (files[0]) {
                if (logLevel == 3 ){
                  console.log(`Restant  ${files.length}`, files[0]);
                } else if (logLevel == 2 ){
                    if (files.length % 10 == 0) {
                      console.log(`Restant  ${files.length}`, files[0]);
                    }
                } else if (logLevel == 1 ){
                  if (files.length % 100 == 0) {
                    console.log(`Restant  ${files.length}`, files[0]);
                  }
              }
              
              worker.send({ file: files[0] });
              files.splice(0, 1);
            } else {
              worker.send({ exit: true });
            }
          });
        });
      } else {
        process.on("message", async message => {
          if (message.exit) {
            process.exit();
          } else {
            // console.log(message);
            const fileName = message.file;
          
  
            try {
              const data = await toGeojson.fromCompressed(fileName);
              if (logLevel > 2){
                if (data.errors.length > 0) {
                    console.log(fileName);
                    console.log(JSON.stringify(data.errors));
                  }
              }

              let dep;
              if (userDep == 'fromFile'){
              
                const basename = path.basename(fileName)
                const bnSplited = basename.split("-");
                if (bnSplited[0] !== 'edigeo'){
                  console.error('le nom du fichier doit être sous la forme "edigeo-DEP....tar.gz" ', fileName);
                  dep = '00';
                }
                else {
                  dep = bnSplited[1].substr(0,2);
                }
              }
              else if (userDep){
                dep = userDep.slice(0,2)
              }
              else {
                if (!depFeatures){
              
                  // depFeatures = JSON.parse(fs.readFileSync(path.join(__dirname,'dep',`dep${year}.geojson`), 'utf8')).features;
                  depFeatures = JSON.parse(fs.readFileSync(path.join(__dirname,'dep',`dep2019.geojson`), 'utf8')).features;
                  flatbushIndex = new Flatbush(depFeatures.length);
                  for (const p of depFeatures) {
                      let bb = p.bbox
                      flatbushIndex.add(bb[0], bb[1], bb[2], bb[3]);
                  }
                  flatbushIndex.finish();
                }
                dep = getDep(data.geojsons.SECTION_id, flatbushIndex, depFeatures); 
                if (dep){
                  dep = dep.slice(0,2); // eg 973 etc
                }else {
                  console.error('pas de dep trouvé!')
                  dep = '00';
                }
              }
            
          
              const commune_id = data.geojsons.COMMUNE_id.features[0].properties.IDU_id;
              const section_id = data.geojsons.SECTION_id.features[0].properties.IDU_id;
              const consts = { commune_id: commune_id, section_id:section_id}
             
              const preparedData = prepareData(config, data,consts, dep, year, epsg)
              await db.pushGeojsonToPostgis(config, preparedData, schema, epsg, pgConfig, withRelations)
           
            } catch (error) {
              console.log("fileName", fileName);
              console.log(error);
            }
            process.send({ pid: process.pid, file: fileName, finish: true });
          }
        });
      }
    });
  };

  
  module.exports = runImport;