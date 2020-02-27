const pgp = require('pg-promise')({
    capSQL: true // generate capitalized SQL 
});

var client = undefined;



const dbConnect = async (pgConfig) => {

      client = pgp(pgConfig)
      return client;

}



const generateTable = async (client, config,  schema, epsg, dropSchema, dropTable ) => {
    const sqlCreateTable = generateSql(config, schema, epsg, dropSchema, dropTable );
    try {
        await client.none(sqlCreateTable);
    } catch (error) {
            console.log(error);
            throw error
    }
    
    return client

}
  

const generateSql = (config, schema, epsg, dropSchema, dropTable,)  => {
    let fkBlocks = [];

    let blocksSql = [ ];
    if (dropSchema){
        blocksSql = [...blocksSql, `DROP SCHEMA IF EXISTS ${schema} CASCADE;`]
    }
    blocksSql = [...blocksSql, `CREATE SCHEMA IF NOT EXISTS ${schema};` ]

   
    for (let idTable in config.tableConfig ){
        const confTable = config.tableConfig[idTable]
        let tableCol = [];
        const table = `${schema}.${confTable.table}`;

            if (confTable.insertGid){
                tableCol.push( `gid serial NOT NULL` )
            }
  
            const confFields = confTable.fields;
            for (let f of confFields){
                tableCol.push( `${f.db} ${f.pgtype}` )
            }

            if (confTable.hashGeom){
                tableCol.push( `geomhash bytea` )
            };
          

            if (confTable.geomField && confTable.geomField.name){
                const geometryType = 'GEOMETRY'
                tableCol.push( `${confTable.geomField.name} geometry(${geometryType}, ${epsg}) NOT NULL` )
            }
            

            if (confTable.pgCONSTRAINT){
                tableCol = [...tableCol, ...confTable.pgCONSTRAINT ]
            }
        
       const strDropTable = `DROP TABLE IF EXISTS ${table} CASCADE;`  
       const strCreateTable = `CREATE TABLE IF NOT EXISTS  ${table} (${tableCol.join(',\n')});`
                                

        let sqlBlockTable = [];
        if (dropTable){
            sqlBlockTable = [...sqlBlockTable, strDropTable]
        }
        
        sqlBlockTable = [...sqlBlockTable, strCreateTable]

        // spatial index
        if (confTable.geomField && confTable.geomField.name){
            sqlBlockTable = [...sqlBlockTable, `CREATE INDEX IF NOT EXISTS spidx_${confTable.geomField.name}_${table.replace('.','_')} ON ${table} USING gist (${confTable.geomField.name});`]
        }

        if (confTable.pgFkCONSTRAINT){  
            fkBlocks = [...fkBlocks, ...confTable.pgFkCONSTRAINT.map( t => t.replace(/\$schema\$/g, schema))]
        }
        blocksSql.push(sqlBlockTable.join('\n\n'))
    }

    blocksSql = [...blocksSql, ...fkBlocks]
    const strResult = blocksSql.join('\n  \n')
    return strResult;
}


const getCs = (config, schema, _table, epsg) => {
    const fields = config.fields.map( f => f.db)
    fields.push({
        name: 'geom',
        mod: '^', // format as raw text
        })
    const table = new pgp.helpers.TableName({table: _table, schema: schema});
    const cs = new pgp.helpers.ColumnSet(fields, {table: table});
    return cs;
}

const pushGeojsonToPostgis = async (configs, data, schema, epsg, pgConfig, withRelations) => {
    if (!client){
        client = await dbConnect(pgConfig);
    }

    for (const tableName in data.geojsons) {
        let conf;
        for (let confId in configs.tableConfig){
            if (configs.tableConfig[confId].table == tableName){
                conf = configs.tableConfig[confId];
            }
        }
        const cs =  getCs(conf, schema, tableName, epsg )
        const confFields = conf.fields.map( f => f.db);
        const features = data.geojsons[tableName].features;
            const dataFeatures = [];
           for (const feature of features) {
            if (!feature.geometry || (feature.geometry && !feature.geometry.type) ) {
                continue;
            }
            let prop = {}
            for (let f of confFields){
                prop[f] = feature.properties[f] || null
            }
            
            let geom = `ST_SetSRID( ST_GeomFromGeoJSON('${JSON.stringify(feature.geometry)}') ,${epsg})`;
            prop['geom'] = geom
          
    
            dataFeatures.push(prop)
            }
            
        const insert = pgp.helpers.insert(dataFeatures, cs) + 'ON CONFLICT DO NOTHING';

        try {
            await client.none(insert);
        } catch (error) {
            console.log(dataFeatures);
            console.log('errrrrrrrrrrrrrroooooooooor', error)
        }
    }
}



module.exports = {generateSql: generateSql, pushGeojsonToPostgis: pushGeojsonToPostgis, dbConnect , generateTable};