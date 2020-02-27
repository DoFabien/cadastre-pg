# cadastre-pg

Script 100% Node Js permettant d'importer le cadastre en EDIGEO vers Postgis.
Il peut être utilisé en ligne de commande (CLI) ou dans un script Node js.

C'est sans doute le plus rapide et le plus fiable d'après ce que j'ai pu tester.

Il utilise :

- [edigeoToGeojson](https://github.com/DoFabien/edigeoToGeojson) pour convertir les données edigeo en geojson.
- [proj-geojson](https://github.com/DoFabien/proj-geojson) pour éventuelement reprojeter les données à la volée
- [decompress](https://github.com/kevva/decompress) pour décompresser les archives .tar.bz2
- [flatbush](https://github.com/mourner/flatbush) & [turf](https://github.com/Turfjs/turf) pour éventuelement déterminer le code département de la feuille cadastrale
- [pg-promise](https://github.com/vitaly-t/pg-promise) pour envoyer efficacement les données vers Postgresql

## Usage (CLI)

### Installation

```sh
npm install -g cadastre-pg
```

### Utilisation

Il prend comme input le chemin du répértoire qui contient les feuilles cadastrales au format __.tar.bz2__ que l'on peut télécherger [ici](https://cadastre.data.gouv.fr/datasets/plan-cadastral-informatise)

Le code du département peut être indiqué dans les options.
Il est possible de lui passer la chaine "fromFile", le code du déparetement sera alors déterminé selon le nom du fichier (celui ci devant être sous la forme "edigeo-{dep}...")
Sinon celui ci sera détérminé automatiquement pour chaque feuille par une opération spatiale (à partir des contours des départements de 2019).

Le système de projection des données __sortantes__ peut être indiqué (à defaut : 4326).

La configuration des tables ainsi que le mapping des champs peut être configuré en lui passant le chemin de son propre fichier de configuration (flag _--config_).
Vous pouvez egalement lui indiquer l'identifiant de l'une des 3 configurations prédéfinies (_full_ / _light_ / _bati_ : elles se trouvent dans le repértoire [_config_](./config/))

```sh
 cadastre-pg [options]
```

##### Options

```sh

    '--path': String, # obligatoire, chemin du répértoire contenant les tar.bz2
    '--year': Number, # obligatoire, année de l'édition de l'EDIGEO
    '--srid':    Number, # SRID de destination defaut : 4326
    '--config': String, # chemin ou identifiant du preset ( defaut "full")
    '--threads' : Number, # defaut max
    '--dep': String, # ex 38, si manquant le script va le determiné geographiquement. "fromFile" si le nom du fichier est sous la forme "edigeo-dep...."
     '--schema': String, # schema de destination ( par defaut : public)

    '--logLevel': Number, # (0, 1, 2 3) verbosité 
    '--dropSchema' : Boolean, # supprime le schema avant l'import
    '--dropTable' : Boolean, # supprime les tables utilisées avant l'import

    '--host':    String,  ## postgresql host (localhost)
    '--database': String, ## postgresql database (postgres)
    '--user': String,     ## postgresql user (postgres)
    '--password': String, ## postgresql password (null)
    '--port':    Number,  ## postgresql password (5432)

```

#### Exemple

```sh
cadastre-pg --path "/data/EDIGEO/dep2A"  --srid 2154 --config "full" --schema "cadastre" -y 2019 --dep "fromFile" --logLevel 2 --dropSchema --host "localhost" --database "gis" --user "fabien" --port 5432 --password "password"
```

## Usage (Node.js)

### Installation

```sh

npm install cadastre-pg
```

```js
cadastrePg(edigeo-path, configuration, pgconfig, options)
```

###### edigeo-path

Obligatoire
Chemin absolut ou relatif du repértoire contenant les fichiers EDIGEO compressé en .tar.bz2

###### configuration

Par défaut, "full" qui fait référence au fichier [./config/full.json]("./config/full.json").
Ce fichier de configuration, en json, permet la création des tables ainsi que le "mapping" des champs.
Il permet également de modifier les données à la volée
Il est possible de lui donner un chemin vers un autre fichier de configuration (en json) ou de lui passer directement l'objet.

###### pgConfig

```json
{
  "user": "postgres",
  "host": "localhost",
  "database": "postgres",
  "password": "password",
  "port": 5432
};
```

###### options :

```json
{
    "schema": "public",
    "year": 2019, // requis => année de la donnée
    "srid": 4326, 
    "threads" : 8, // nombre de theards aloués (defaut = max)
    "codeDep" : null,
    "logLevel": 2, //0, 1, 2 , 3 
    "dropSchema": false, // supprime les tables concernées avant l'import
    "dropTable": false, // supprime le schema avant l'import
}

```

### Exemple

```js
const cadastrePg = require('cadastre-pg');

let options = {
    "schema": "cadastre_test",
    "year": 2019,
    "srid": 2154,
    "threads" : 8,
    "codeDep" : null,
    "logLevel": 2,
    "dropSchema": true,
    "dropTable": true
}

let  pgConfig = {
  "user": "fabien",
  "host": "localhost",
  "database": "gis",
  "password": "password",
  "port": 5432
};

// chemin des dossiers contenant les fichier EDIGEO compressé en bz
cadastrePg('/data/dep38/', 'full', pgConfig , options)
.then( t => { 
    console.log('fini')
    })
.catch(error => {
  console.log(error);
})

```

## Benchmark

Je fais ces tests sur un PC relativement puissant sous la dernière monture d'Ubuntu avec node 12.16

PC

- CPU : AMD Ryzen 2700X (8 coeurs, 16th)
- 16Go de RAM
- Un SSD

Système :

- Ubuntu 19.10
- Node 12.16
- Postgres 12 ( [docker](https://hub.docker.com/r/kartoza/postgis/) )

Les temps indiqués prennent en compte la décompression de l'archive en tar.bz2
On utilise tous les threads du CPU.

```sh
cadastre-pg --path "/data/EDIGEO/dep2A"  --srid 2154/4326 --config "full" --schema "cadastre" -y 2019 --dep "fromFile" --logLevel 0 --dropSchema --host "localhost" --database "gis" --user "fabien" --port 5432 --password "password"
```

| Département | srid : 2154 (sans reprojection) |  srid : 4326  |
| ------ | ----------- |----------- |
| 2A (189 Mo)   | 53 s | 86 s |
| 2B (367 Mo) | 93 s| 168 s |
| 38 (722 Mo)   | 230 s | 330 s |
