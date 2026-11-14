// Writes to the names of a delegate's context (Qt 6.12): a role and `model.role` write to the
// model and tell the bindings that read them, `index` is read-only, a new name on `model` is
// kept by a row that is no ListModel row and dropped by one that is. Every kind of model in
// every consumer; the last four numbers of a line count the evaluations of the bindings on the
// role, on `modelData`, on `model.role` and on `index`.
// qt: rep lm start -> string:b,string:b,undefined,undefined,NoProp(name),number:1,number:1,string:b|2,3/3/3/3;rep lm name= ok -> string:z,string:z,undefined,undefined,NoProp(name),number:1,number:1,string:z|2,4/4/4/3;rep lm model.name= ok -> string:m,string:m,undefined,undefined,NoProp(name),number:1,number:1,string:m|2,5/5/5/3;rep lm modelData.name= TypeError: Value is undefined and could not be converted to an object -> string:m,string:m,undefined,undefined,NoProp(name),number:1,number:1,string:m|2,5/5/5/3;rep lm model.modelData.name= TypeError: Value is undefined and could not be converted to an object -> string:m,string:m,undefined,undefined,NoProp(name),number:1,number:1,string:m|2,5/5/5/3;rep lm modelData= ok -> string:m,string:m,undefined,undefined,NoProp(name),number:1,number:1,string:m|2,5/5/5/3;rep lm model.modelData= ok -> string:m,string:m,undefined,undefined,NoProp(name),number:1,number:1,string:m|2,5/5/5/3;rep lm index= TypeError: Cannot assign to read-only property "index" -> string:m,string:m,undefined,undefined,NoProp(name),number:1,number:1,string:m|2,5/5/5/3;rep lm model.index= TypeError: Cannot assign to read-only property "index" -> string:m,string:m,undefined,undefined,NoProp(name),number:1,number:1,string:m|2,5/5/5/3;rep lm model.nope= ok -> string:m,string:m,undefined,undefined,NoProp(name),number:1,number:1,string:m|2,5/5/5/3;rep lm strict name= ok -> string:y,string:y,undefined,undefined,NoProp(name),number:1,number:1,string:y|2,6/6/6/3;rep lm strict model.nope= ok -> string:y,string:y,undefined,undefined,NoProp(name),number:1,number:1,string:y|2,6/6/6/3;rep lm n= ok -> string:y,string:y,undefined,undefined,NoProp(name),number:1,number:1,string:y|0,6/7/6/3;rep lm model.n= ok -> string:y,string:y,undefined,undefined,NoProp(name),number:1,number:1,string:y|7,6/8/6/3;rep lm d.name= Error: Cannot assign to non-existent property "name" -> string:y,string:y,undefined,undefined,NoProp(name),number:1,number:1,string:y|7,6/8/6/3;lv lm start -> string:b,string:b,undefined,undefined,NoProp(name),number:1,number:1,string:b|2,3/3/3/3;lv lm name= ok -> string:z,string:z,undefined,undefined,NoProp(name),number:1,number:1,string:z|2,4/4/4/3;lv lm model.name= ok -> string:m,string:m,undefined,undefined,NoProp(name),number:1,number:1,string:m|2,5/5/5/3;lv lm modelData.name= TypeError: Value is undefined and could not be converted to an object -> string:m,string:m,undefined,undefined,NoProp(name),number:1,number:1,string:m|2,5/5/5/3;lv lm model.modelData.name= TypeError: Value is undefined and could not be converted to an object -> string:m,string:m,undefined,undefined,NoProp(name),number:1,number:1,string:m|2,5/5/5/3;lv lm modelData= ok -> string:m,string:m,undefined,undefined,NoProp(name),number:1,number:1,string:m|2,5/5/5/3;lv lm model.modelData= ok -> string:m,string:m,undefined,undefined,NoProp(name),number:1,number:1,string:m|2,5/5/5/3;lv lm index= TypeError: Cannot assign to read-only property "index" -> string:m,string:m,undefined,undefined,NoProp(name),number:1,number:1,string:m|2,5/5/5/3;lv lm model.index= TypeError: Cannot assign to read-only property "index" -> string:m,string:m,undefined,undefined,NoProp(name),number:1,number:1,string:m|2,5/5/5/3;lv lm model.nope= ok -> string:m,string:m,undefined,undefined,NoProp(name),number:1,number:1,string:m|2,5/5/5/3;lv lm strict name= ok -> string:y,string:y,undefined,undefined,NoProp(name),number:1,number:1,string:y|2,6/6/6/3;lv lm strict model.nope= ok -> string:y,string:y,undefined,undefined,NoProp(name),number:1,number:1,string:y|2,6/6/6/3;lv lm n= ok -> string:y,string:y,undefined,undefined,NoProp(name),number:1,number:1,string:y|0,6/7/6/3;lv lm model.n= ok -> string:y,string:y,undefined,undefined,NoProp(name),number:1,number:1,string:y|7,6/8/6/3;lv lm d.name= Error: Cannot assign to non-existent property "name" -> string:y,string:y,undefined,undefined,NoProp(name),number:1,number:1,string:y|7,6/8/6/3;gv lm start -> string:b,string:b,undefined,undefined,NoProp(name),number:1,number:1,string:b|2,3/3/3/3;gv lm name= ok -> string:z,string:z,undefined,undefined,NoProp(name),number:1,number:1,string:z|2,4/4/4/3;gv lm model.name= ok -> string:m,string:m,undefined,undefined,NoProp(name),number:1,number:1,string:m|2,5/5/5/3;gv lm modelData.name= TypeError: Value is undefined and could not be converted to an object -> string:m,string:m,undefined,undefined,NoProp(name),number:1,number:1,string:m|2,5/5/5/3;gv lm model.modelData.name= TypeError: Value is undefined and could not be converted to an object -> string:m,string:m,undefined,undefined,NoProp(name),number:1,number:1,string:m|2,5/5/5/3;gv lm modelData= ok -> string:m,string:m,undefined,undefined,NoProp(name),number:1,number:1,string:m|2,5/5/5/3;gv lm model.modelData= ok -> string:m,string:m,undefined,undefined,NoProp(name),number:1,number:1,string:m|2,5/5/5/3;gv lm index= TypeError: Cannot assign to read-only property "index" -> string:m,string:m,undefined,undefined,NoProp(name),number:1,number:1,string:m|2,5/5/5/3;gv lm model.index= TypeError: Cannot assign to read-only property "index" -> string:m,string:m,undefined,undefined,NoProp(name),number:1,number:1,string:m|2,5/5/5/3;gv lm model.nope= ok -> string:m,string:m,undefined,undefined,NoProp(name),number:1,number:1,string:m|2,5/5/5/3;gv lm strict name= ok -> string:y,string:y,undefined,undefined,NoProp(name),number:1,number:1,string:y|2,6/6/6/3;gv lm strict model.nope= ok -> string:y,string:y,undefined,undefined,NoProp(name),number:1,number:1,string:y|2,6/6/6/3;gv lm n= ok -> string:y,string:y,undefined,undefined,NoProp(name),number:1,number:1,string:y|0,6/7/6/3;gv lm model.n= ok -> string:y,string:y,undefined,undefined,NoProp(name),number:1,number:1,string:y|7,6/8/6/3;gv lm d.name= Error: Cannot assign to non-existent property "name" -> string:y,string:y,undefined,undefined,NoProp(name),number:1,number:1,string:y|7,6/8/6/3;inst lm start -> string:b,string:b,undefined,undefined,NoProp(name),number:1,number:1,string:b|2,3/3/3/3;inst lm name= ok -> string:z,string:z,undefined,undefined,NoProp(name),number:1,number:1,string:z|2,4/4/4/3;inst lm model.name= ok -> string:m,string:m,undefined,undefined,NoProp(name),number:1,number:1,string:m|2,5/5/5/3;inst lm modelData.name= TypeError: Value is undefined and could not be converted to an object -> string:m,string:m,undefined,undefined,NoProp(name),number:1,number:1,string:m|2,5/5/5/3;inst lm model.modelData.name= TypeError: Value is undefined and could not be converted to an object -> string:m,string:m,undefined,undefined,NoProp(name),number:1,number:1,string:m|2,5/5/5/3;inst lm modelData= ok -> string:m,string:m,undefined,undefined,NoProp(name),number:1,number:1,string:m|2,5/5/5/3;inst lm model.modelData= ok -> string:m,string:m,undefined,undefined,NoProp(name),number:1,number:1,string:m|2,5/5/5/3;inst lm index= TypeError: Cannot assign to read-only property "index" -> string:m,string:m,undefined,undefined,NoProp(name),number:1,number:1,string:m|2,5/5/5/3;inst lm model.index= TypeError: Cannot assign to read-only property "index" -> string:m,string:m,undefined,undefined,NoProp(name),number:1,number:1,string:m|2,5/5/5/3;inst lm model.nope= ok -> string:m,string:m,undefined,undefined,NoProp(name),number:1,number:1,string:m|2,5/5/5/3;inst lm strict name= ok -> string:y,string:y,undefined,undefined,NoProp(name),number:1,number:1,string:y|2,6/6/6/3;inst lm strict model.nope= ok -> string:y,string:y,undefined,undefined,NoProp(name),number:1,number:1,string:y|2,6/6/6/3;inst lm n= ok -> string:y,string:y,undefined,undefined,NoProp(name),number:1,number:1,string:y|0,6/7/6/3;inst lm model.n= ok -> string:y,string:y,undefined,undefined,NoProp(name),number:1,number:1,string:y|7,6/8/6/3;inst lm d.name= Error: Cannot assign to non-existent property "name" -> string:y,string:y,undefined,undefined,NoProp(name),number:1,number:1,string:y|7,6/8/6/3;rep lm1 start -> string:b,string:b,string:b,string:b,undefined,number:1,number:1,string:b,3/3/3/3;rep lm1 name= ok -> string:z,string:z,string:z,string:z,undefined,number:1,number:1,string:z,4/4/4/3;rep lm1 model.name= ok -> string:m,string:m,string:m,string:m,undefined,number:1,number:1,string:m,5/5/5/3;rep lm1 modelData.name= ok -> string:m,string:m,string:m,string:m,undefined,number:1,number:1,string:m,5/5/5/3;rep lm1 model.modelData.name= ok -> string:m,string:m,string:m,string:m,undefined,number:1,number:1,string:m,5/5/5/3;rep lm1 modelData= ok -> string:q,string:q,string:q,string:q,undefined,number:1,number:1,string:q,7/7/7/3;rep lm1 model.modelData= ok -> string:r,string:r,string:r,string:r,undefined,number:1,number:1,string:r,9/9/9/3;rep lm1 index= TypeError: Cannot assign to read-only property "index" -> string:r,string:r,string:r,string:r,undefined,number:1,number:1,string:r,9/9/9/3;rep lm1 model.index= TypeError: Cannot assign to read-only property "index" -> string:r,string:r,string:r,string:r,undefined,number:1,number:1,string:r,9/9/9/3;rep lm1 model.nope= ok -> string:r,string:r,string:r,string:r,undefined,number:1,number:1,string:r,9/9/9/3;rep lm1 strict name= ok -> string:y,string:y,string:y,string:y,undefined,number:1,number:1,string:y,10/10/10/3;rep lm1 strict model.nope= ok -> string:y,string:y,string:y,string:y,undefined,number:1,number:1,string:y,10/10/10/3;rep lm1 n= Error: Invalid write to global property "n" -> string:y,string:y,string:y,string:y,undefined,number:1,number:1,string:y,10/10/10/3;rep lm1 model.n= ok -> string:y,string:y,string:y,string:y,undefined,number:1,number:1,string:y,10/10/10/3;rep lm1 d.name= Error: Cannot assign to non-existent property "name" -> string:y,string:y,string:y,string:y,undefined,number:1,number:1,string:y,10/10/10/3;lv lm1 start -> string:b,string:b,string:b,string:b,undefined,number:1,number:1,string:b,3/3/3/3;lv lm1 name= ok -> string:z,string:z,string:z,string:z,undefined,number:1,number:1,string:z,4/4/4/3;lv lm1 model.name= ok -> string:m,string:m,string:m,string:m,undefined,number:1,number:1,string:m,5/5/5/3;lv lm1 modelData.name= ok -> string:m,string:m,string:m,string:m,undefined,number:1,number:1,string:m,5/5/5/3;lv lm1 model.modelData.name= ok -> string:m,string:m,string:m,string:m,undefined,number:1,number:1,string:m,5/5/5/3;lv lm1 modelData= ok -> string:q,string:q,string:q,string:q,undefined,number:1,number:1,string:q,7/7/7/3;lv lm1 model.modelData= ok -> string:r,string:r,string:r,string:r,undefined,number:1,number:1,string:r,9/9/9/3;lv lm1 index= TypeError: Cannot assign to read-only property "index" -> string:r,string:r,string:r,string:r,undefined,number:1,number:1,string:r,9/9/9/3;lv lm1 model.index= TypeError: Cannot assign to read-only property "index" -> string:r,string:r,string:r,string:r,undefined,number:1,number:1,string:r,9/9/9/3;lv lm1 model.nope= ok -> string:r,string:r,string:r,string:r,undefined,number:1,number:1,string:r,9/9/9/3;lv lm1 strict name= ok -> string:y,string:y,string:y,string:y,undefined,number:1,number:1,string:y,10/10/10/3;lv lm1 strict model.nope= ok -> string:y,string:y,string:y,string:y,undefined,number:1,number:1,string:y,10/10/10/3;lv lm1 n= Error: Invalid write to global property "n" -> string:y,string:y,string:y,string:y,undefined,number:1,number:1,string:y,10/10/10/3;lv lm1 model.n= ok -> string:y,string:y,string:y,string:y,undefined,number:1,number:1,string:y,10/10/10/3;lv lm1 d.name= Error: Cannot assign to non-existent property "name" -> string:y,string:y,string:y,string:y,undefined,number:1,number:1,string:y,10/10/10/3;gv lm1 start -> string:b,string:b,string:b,string:b,undefined,number:1,number:1,string:b,3/3/3/3;gv lm1 name= ok -> string:z,string:z,string:z,string:z,undefined,number:1,number:1,string:z,4/4/4/3;gv lm1 model.name= ok -> string:m,string:m,string:m,string:m,undefined,number:1,number:1,string:m,5/5/5/3;gv lm1 modelData.name= ok -> string:m,string:m,string:m,string:m,undefined,number:1,number:1,string:m,5/5/5/3;gv lm1 model.modelData.name= ok -> string:m,string:m,string:m,string:m,undefined,number:1,number:1,string:m,5/5/5/3;gv lm1 modelData= ok -> string:q,string:q,string:q,string:q,undefined,number:1,number:1,string:q,7/7/7/3;gv lm1 model.modelData= ok -> string:r,string:r,string:r,string:r,undefined,number:1,number:1,string:r,9/9/9/3;gv lm1 index= TypeError: Cannot assign to read-only property "index" -> string:r,string:r,string:r,string:r,undefined,number:1,number:1,string:r,9/9/9/3;gv lm1 model.index= TypeError: Cannot assign to read-only property "index" -> string:r,string:r,string:r,string:r,undefined,number:1,number:1,string:r,9/9/9/3;gv lm1 model.nope= ok -> string:r,string:r,string:r,string:r,undefined,number:1,number:1,string:r,9/9/9/3;gv lm1 strict name= ok -> string:y,string:y,string:y,string:y,undefined,number:1,number:1,string:y,10/10/10/3;gv lm1 strict model.nope= ok -> string:y,string:y,string:y,string:y,undefined,number:1,number:1,string:y,10/10/10/3;gv lm1 n= Error: Invalid write to global property "n" -> string:y,string:y,string:y,string:y,undefined,number:1,number:1,string:y,10/10/10/3;gv lm1 model.n= ok -> string:y,string:y,string:y,string:y,undefined,number:1,number:1,string:y,10/10/10/3;gv lm1 d.name= Error: Cannot assign to non-existent property "name" -> string:y,string:y,string:y,string:y,undefined,number:1,number:1,string:y,10/10/10/3;inst lm1 start -> string:b,string:b,string:b,string:b,undefined,number:1,number:1,string:b,3/3/3/3;inst lm1 name= ok -> string:z,string:z,string:z,string:z,undefined,number:1,number:1,string:z,4/4/4/3;inst lm1 model.name= ok -> string:m,string:m,string:m,string:m,undefined,number:1,number:1,string:m,5/5/5/3;inst lm1 modelData.name= ok -> string:m,string:m,string:m,string:m,undefined,number:1,number:1,string:m,5/5/5/3;inst lm1 model.modelData.name= ok -> string:m,string:m,string:m,string:m,undefined,number:1,number:1,string:m,5/5/5/3;inst lm1 modelData= ok -> string:q,string:q,string:q,string:q,undefined,number:1,number:1,string:q,7/7/7/3;inst lm1 model.modelData= ok -> string:r,string:r,string:r,string:r,undefined,number:1,number:1,string:r,9/9/9/3;inst lm1 index= TypeError: Cannot assign to read-only property "index" -> string:r,string:r,string:r,string:r,undefined,number:1,number:1,string:r,9/9/9/3;inst lm1 model.index= TypeError: Cannot assign to read-only property "index" -> string:r,string:r,string:r,string:r,undefined,number:1,number:1,string:r,9/9/9/3;inst lm1 model.nope= ok -> string:r,string:r,string:r,string:r,undefined,number:1,number:1,string:r,9/9/9/3;inst lm1 strict name= ok -> string:y,string:y,string:y,string:y,undefined,number:1,number:1,string:y,10/10/10/3;inst lm1 strict model.nope= ok -> string:y,string:y,string:y,string:y,undefined,number:1,number:1,string:y,10/10/10/3;inst lm1 n= Error: Invalid write to global property "n" -> string:y,string:y,string:y,string:y,undefined,number:1,number:1,string:y,10/10/10/3;inst lm1 model.n= ok -> string:y,string:y,string:y,string:y,undefined,number:1,number:1,string:y,10/10/10/3;inst lm1 d.name= Error: Cannot assign to non-existent property "name" -> string:y,string:y,string:y,string:y,undefined,number:1,number:1,string:y,10/10/10/3;rep num start -> RefErr(name),undefined,number:1,number:1,undefined,number:1,number:1,string:-,3/3/3/3;rep num name= Error: Invalid write to global property "name" -> RefErr(name),undefined,number:1,number:1,undefined,number:1,number:1,string:-,3/3/3/3;rep num model.name= ok -> string:m,string:m,number:1,number:1,undefined,number:1,number:1,string:-,3/3/3/3;rep num modelData.name= ok -> string:m,string:m,number:1,number:1,undefined,number:1,number:1,string:-,3/3/3/3;rep num model.modelData.name= ok -> string:m,string:m,number:1,number:1,undefined,number:1,number:1,string:-,3/3/3/3;rep num modelData= ok -> string:m,string:m,string:q,string:q,undefined,number:1,number:1,string:-,3/4/3/3;rep num model.modelData= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:-,3/5/3/3;rep num index= TypeError: Cannot assign to read-only property "index" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:-,3/5/3/3;rep num model.index= TypeError: Cannot assign to read-only property "index" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:-,3/5/3/3;rep num model.nope= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:-,3/5/3/3;rep num strict name= Error: Invalid write to global property "name" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:-,3/5/3/3;rep num strict model.nope= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:-,3/5/3/3;rep num n= Error: Invalid write to global property "n" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:-,3/5/3/3;rep num model.n= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:-,3/5/3/3;rep num d.name= Error: Cannot assign to non-existent property "name" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:-,3/5/3/3;lv num start -> RefErr(name),undefined,number:1,number:1,undefined,number:1,number:1,string:-,3/3/3/3;lv num name= Error: Invalid write to global property "name" -> RefErr(name),undefined,number:1,number:1,undefined,number:1,number:1,string:-,3/3/3/3;lv num model.name= ok -> string:m,string:m,number:1,number:1,undefined,number:1,number:1,string:-,3/3/3/3;lv num modelData.name= ok -> string:m,string:m,number:1,number:1,undefined,number:1,number:1,string:-,3/3/3/3;lv num model.modelData.name= ok -> string:m,string:m,number:1,number:1,undefined,number:1,number:1,string:-,3/3/3/3;lv num modelData= ok -> string:m,string:m,string:q,string:q,undefined,number:1,number:1,string:-,3/4/3/3;lv num model.modelData= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:-,3/5/3/3;lv num index= TypeError: Cannot assign to read-only property "index" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:-,3/5/3/3;lv num model.index= TypeError: Cannot assign to read-only property "index" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:-,3/5/3/3;lv num model.nope= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:-,3/5/3/3;lv num strict name= Error: Invalid write to global property "name" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:-,3/5/3/3;lv num strict model.nope= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:-,3/5/3/3;lv num n= Error: Invalid write to global property "n" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:-,3/5/3/3;lv num model.n= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:-,3/5/3/3;lv num d.name= Error: Cannot assign to non-existent property "name" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:-,3/5/3/3;gv num start -> RefErr(name),undefined,number:1,number:1,undefined,number:1,number:1,string:-,3/3/3/3;gv num name= Error: Invalid write to global property "name" -> RefErr(name),undefined,number:1,number:1,undefined,number:1,number:1,string:-,3/3/3/3;gv num model.name= ok -> string:m,string:m,number:1,number:1,undefined,number:1,number:1,string:-,3/3/3/3;gv num modelData.name= ok -> string:m,string:m,number:1,number:1,undefined,number:1,number:1,string:-,3/3/3/3;gv num model.modelData.name= ok -> string:m,string:m,number:1,number:1,undefined,number:1,number:1,string:-,3/3/3/3;gv num modelData= ok -> string:m,string:m,string:q,string:q,undefined,number:1,number:1,string:-,3/4/3/3;gv num model.modelData= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:-,3/5/3/3;gv num index= TypeError: Cannot assign to read-only property "index" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:-,3/5/3/3;gv num model.index= TypeError: Cannot assign to read-only property "index" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:-,3/5/3/3;gv num model.nope= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:-,3/5/3/3;gv num strict name= Error: Invalid write to global property "name" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:-,3/5/3/3;gv num strict model.nope= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:-,3/5/3/3;gv num n= Error: Invalid write to global property "n" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:-,3/5/3/3;gv num model.n= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:-,3/5/3/3;gv num d.name= Error: Cannot assign to non-existent property "name" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:-,3/5/3/3;inst num start -> RefErr(name),undefined,number:1,number:1,undefined,number:1,number:1,string:-,3/3/3/3;inst num name= Error: Invalid write to global property "name" -> RefErr(name),undefined,number:1,number:1,undefined,number:1,number:1,string:-,3/3/3/3;inst num model.name= ok -> string:m,string:m,number:1,number:1,undefined,number:1,number:1,string:-,3/3/3/3;inst num modelData.name= ok -> string:m,string:m,number:1,number:1,undefined,number:1,number:1,string:-,3/3/3/3;inst num model.modelData.name= ok -> string:m,string:m,number:1,number:1,undefined,number:1,number:1,string:-,3/3/3/3;inst num modelData= ok -> string:m,string:m,string:q,string:q,undefined,number:1,number:1,string:-,3/4/3/3;inst num model.modelData= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:-,3/5/3/3;inst num index= TypeError: Cannot assign to read-only property "index" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:-,3/5/3/3;inst num model.index= TypeError: Cannot assign to read-only property "index" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:-,3/5/3/3;inst num model.nope= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:-,3/5/3/3;inst num strict name= Error: Invalid write to global property "name" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:-,3/5/3/3;inst num strict model.nope= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:-,3/5/3/3;inst num n= Error: Invalid write to global property "n" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:-,3/5/3/3;inst num model.n= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:-,3/5/3/3;inst num d.name= Error: Cannot assign to non-existent property "name" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:-,3/5/3/3;rep arr start -> RefErr(name),undefined,string:y,string:y,undefined,number:1,number:1,string:y,3/3/3/3;rep arr name= Error: Invalid write to global property "name" -> RefErr(name),undefined,string:y,string:y,undefined,number:1,number:1,string:y,3/3/3/3;rep arr model.name= ok -> string:m,string:m,string:y,string:y,undefined,number:1,number:1,string:y,3/3/3/3;rep arr modelData.name= ok -> string:m,string:m,string:y,string:y,undefined,number:1,number:1,string:y,3/3/3/3;rep arr model.modelData.name= ok -> string:m,string:m,string:y,string:y,undefined,number:1,number:1,string:y,3/3/3/3;rep arr modelData= ok -> string:m,string:m,string:q,string:q,undefined,number:1,number:1,string:y,3/4/3/3;rep arr model.modelData= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:y,3/5/3/3;rep arr index= TypeError: Cannot assign to read-only property "index" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:y,3/5/3/3;rep arr model.index= TypeError: Cannot assign to read-only property "index" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:y,3/5/3/3;rep arr model.nope= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:y,3/5/3/3;rep arr strict name= Error: Invalid write to global property "name" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:y,3/5/3/3;rep arr strict model.nope= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:y,3/5/3/3;rep arr n= Error: Invalid write to global property "n" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:y,3/5/3/3;rep arr model.n= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:y,3/5/3/3;rep arr d.name= Error: Cannot assign to non-existent property "name" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:y,3/5/3/3;lv arr start -> RefErr(name),undefined,string:y,string:y,undefined,number:1,number:1,string:y,3/3/3/3;lv arr name= Error: Invalid write to global property "name" -> RefErr(name),undefined,string:y,string:y,undefined,number:1,number:1,string:y,3/3/3/3;lv arr model.name= ok -> string:m,string:m,string:y,string:y,undefined,number:1,number:1,string:y,3/3/3/3;lv arr modelData.name= ok -> string:m,string:m,string:y,string:y,undefined,number:1,number:1,string:y,3/3/3/3;lv arr model.modelData.name= ok -> string:m,string:m,string:y,string:y,undefined,number:1,number:1,string:y,3/3/3/3;lv arr modelData= ok -> string:m,string:m,string:q,string:q,undefined,number:1,number:1,string:y,3/4/3/3;lv arr model.modelData= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:y,3/5/3/3;lv arr index= TypeError: Cannot assign to read-only property "index" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:y,3/5/3/3;lv arr model.index= TypeError: Cannot assign to read-only property "index" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:y,3/5/3/3;lv arr model.nope= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:y,3/5/3/3;lv arr strict name= Error: Invalid write to global property "name" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:y,3/5/3/3;lv arr strict model.nope= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:y,3/5/3/3;lv arr n= Error: Invalid write to global property "n" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:y,3/5/3/3;lv arr model.n= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:y,3/5/3/3;lv arr d.name= Error: Cannot assign to non-existent property "name" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:y,3/5/3/3;gv arr start -> RefErr(name),undefined,string:y,string:y,undefined,number:1,number:1,string:y,3/3/3/3;gv arr name= Error: Invalid write to global property "name" -> RefErr(name),undefined,string:y,string:y,undefined,number:1,number:1,string:y,3/3/3/3;gv arr model.name= ok -> string:m,string:m,string:y,string:y,undefined,number:1,number:1,string:y,3/3/3/3;gv arr modelData.name= ok -> string:m,string:m,string:y,string:y,undefined,number:1,number:1,string:y,3/3/3/3;gv arr model.modelData.name= ok -> string:m,string:m,string:y,string:y,undefined,number:1,number:1,string:y,3/3/3/3;gv arr modelData= ok -> string:m,string:m,string:q,string:q,undefined,number:1,number:1,string:y,3/4/3/3;gv arr model.modelData= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:y,3/5/3/3;gv arr index= TypeError: Cannot assign to read-only property "index" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:y,3/5/3/3;gv arr model.index= TypeError: Cannot assign to read-only property "index" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:y,3/5/3/3;gv arr model.nope= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:y,3/5/3/3;gv arr strict name= Error: Invalid write to global property "name" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:y,3/5/3/3;gv arr strict model.nope= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:y,3/5/3/3;gv arr n= Error: Invalid write to global property "n" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:y,3/5/3/3;gv arr model.n= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:y,3/5/3/3;gv arr d.name= Error: Cannot assign to non-existent property "name" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:y,3/5/3/3;inst arr start -> RefErr(name),undefined,string:y,string:y,undefined,number:1,number:1,string:y,3/3/3/3;inst arr name= Error: Invalid write to global property "name" -> RefErr(name),undefined,string:y,string:y,undefined,number:1,number:1,string:y,3/3/3/3;inst arr model.name= ok -> string:m,string:m,string:y,string:y,undefined,number:1,number:1,string:y,3/3/3/3;inst arr modelData.name= ok -> string:m,string:m,string:y,string:y,undefined,number:1,number:1,string:y,3/3/3/3;inst arr model.modelData.name= ok -> string:m,string:m,string:y,string:y,undefined,number:1,number:1,string:y,3/3/3/3;inst arr modelData= ok -> string:m,string:m,string:q,string:q,undefined,number:1,number:1,string:y,3/4/3/3;inst arr model.modelData= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:y,3/5/3/3;inst arr index= TypeError: Cannot assign to read-only property "index" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:y,3/5/3/3;inst arr model.index= TypeError: Cannot assign to read-only property "index" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:y,3/5/3/3;inst arr model.nope= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:y,3/5/3/3;inst arr strict name= Error: Invalid write to global property "name" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:y,3/5/3/3;inst arr strict model.nope= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:y,3/5/3/3;inst arr n= Error: Invalid write to global property "n" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:y,3/5/3/3;inst arr model.n= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:y,3/5/3/3;inst arr d.name= Error: Cannot assign to non-existent property "name" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:y,3/5/3/3;rep arro start -> RefErr(name),undefined,object,object,string:b,number:1,number:1,string:b|2,3/3/3/3;rep arro name= Error: Invalid write to global property "name" -> RefErr(name),undefined,object,object,string:b,number:1,number:1,string:b|2,3/3/3/3;rep arro model.name= ok -> string:m,string:m,object,object,string:b,number:1,number:1,string:b|2,3/3/3/3;rep arro modelData= ok -> string:m,string:m,string:q,string:q,undefined,number:1,number:1,string:b|2,3/4/3/3;rep arro model.modelData= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:b|2,3/5/3/3;rep arro index= TypeError: Cannot assign to read-only property "index" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:b|2,3/5/3/3;rep arro model.index= TypeError: Cannot assign to read-only property "index" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:b|2,3/5/3/3;rep arro model.nope= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:b|2,3/5/3/3;rep arro strict name= Error: Invalid write to global property "name" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:b|2,3/5/3/3;rep arro strict model.nope= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:b|2,3/5/3/3;rep arro n= Error: Invalid write to global property "n" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:b|2,3/5/3/3;rep arro model.n= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:b|2,3/5/3/3;rep arro d.name= Error: Cannot assign to non-existent property "name" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:b|2,3/5/3/3;lv arro start -> RefErr(name),undefined,object,object,string:b,number:1,number:1,string:b|2,3/3/3/3;lv arro name= Error: Invalid write to global property "name" -> RefErr(name),undefined,object,object,string:b,number:1,number:1,string:b|2,3/3/3/3;lv arro model.name= ok -> string:m,string:m,object,object,string:b,number:1,number:1,string:b|2,3/3/3/3;lv arro modelData= ok -> string:m,string:m,string:q,string:q,undefined,number:1,number:1,string:b|2,3/4/3/3;lv arro model.modelData= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:b|2,3/5/3/3;lv arro index= TypeError: Cannot assign to read-only property "index" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:b|2,3/5/3/3;lv arro model.index= TypeError: Cannot assign to read-only property "index" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:b|2,3/5/3/3;lv arro model.nope= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:b|2,3/5/3/3;lv arro strict name= Error: Invalid write to global property "name" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:b|2,3/5/3/3;lv arro strict model.nope= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:b|2,3/5/3/3;lv arro n= Error: Invalid write to global property "n" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:b|2,3/5/3/3;lv arro model.n= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:b|2,3/5/3/3;lv arro d.name= Error: Cannot assign to non-existent property "name" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:b|2,3/5/3/3;gv arro start -> RefErr(name),undefined,object,object,string:b,number:1,number:1,string:b|2,3/3/3/3;gv arro name= Error: Invalid write to global property "name" -> RefErr(name),undefined,object,object,string:b,number:1,number:1,string:b|2,3/3/3/3;gv arro model.name= ok -> string:m,string:m,object,object,string:b,number:1,number:1,string:b|2,3/3/3/3;gv arro modelData= ok -> string:m,string:m,string:q,string:q,undefined,number:1,number:1,string:b|2,3/4/3/3;gv arro model.modelData= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:b|2,3/5/3/3;gv arro index= TypeError: Cannot assign to read-only property "index" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:b|2,3/5/3/3;gv arro model.index= TypeError: Cannot assign to read-only property "index" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:b|2,3/5/3/3;gv arro model.nope= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:b|2,3/5/3/3;gv arro strict name= Error: Invalid write to global property "name" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:b|2,3/5/3/3;gv arro strict model.nope= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:b|2,3/5/3/3;gv arro n= Error: Invalid write to global property "n" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:b|2,3/5/3/3;gv arro model.n= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:b|2,3/5/3/3;gv arro d.name= Error: Cannot assign to non-existent property "name" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:b|2,3/5/3/3;inst arro start -> RefErr(name),undefined,object,object,string:b,number:1,number:1,string:b|2,3/3/3/3;inst arro name= Error: Invalid write to global property "name" -> RefErr(name),undefined,object,object,string:b,number:1,number:1,string:b|2,3/3/3/3;inst arro model.name= ok -> string:m,string:m,object,object,string:b,number:1,number:1,string:b|2,3/3/3/3;inst arro modelData= ok -> string:m,string:m,string:q,string:q,undefined,number:1,number:1,string:b|2,3/4/3/3;inst arro model.modelData= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:b|2,3/5/3/3;inst arro index= TypeError: Cannot assign to read-only property "index" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:b|2,3/5/3/3;inst arro model.index= TypeError: Cannot assign to read-only property "index" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:b|2,3/5/3/3;inst arro model.nope= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:b|2,3/5/3/3;inst arro strict name= Error: Invalid write to global property "name" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:b|2,3/5/3/3;inst arro strict model.nope= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:b|2,3/5/3/3;inst arro n= Error: Invalid write to global property "n" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:b|2,3/5/3/3;inst arro model.n= ok -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:b|2,3/5/3/3;inst arro d.name= Error: Cannot assign to non-existent property "name" -> string:m,string:m,string:r,string:r,undefined,number:1,number:1,string:b|2,3/5/3/3;rep objs start -> string:b,string:b,object,object,string:b,number:1,number:1,string:b|2,3/3/3/3;rep objs name= ok -> string:z,string:z,object,object,string:z,number:1,number:1,string:z|2,4/3/4/3;rep objs model.name= ok -> string:m,string:m,object,object,string:m,number:1,number:1,string:m|2,5/3/5/3;rep objs modelData.name= ok -> string:s,string:s,object,object,string:s,number:1,number:1,string:s|2,6/3/6/3;rep objs model.modelData.name= ok -> string:t,string:t,object,object,string:t,number:1,number:1,string:t|2,7/3/7/3;rep objs modelData= TypeError: Cannot assign to read-only property "modelData" -> string:t,string:t,object,object,string:t,number:1,number:1,string:t|2,7/3/7/3;rep objs model.modelData= TypeError: Cannot assign to read-only property "modelData" -> string:t,string:t,object,object,string:t,number:1,number:1,string:t|2,7/3/7/3;rep objs index= TypeError: Cannot assign to read-only property "index" -> string:t,string:t,object,object,string:t,number:1,number:1,string:t|2,7/3/7/3;rep objs model.index= TypeError: Cannot assign to read-only property "index" -> string:t,string:t,object,object,string:t,number:1,number:1,string:t|2,7/3/7/3;rep objs model.nope= ok -> string:t,string:t,object,object,string:t,number:1,number:1,string:t|2,7/3/7/3;rep objs strict name= ok -> string:y,string:y,object,object,string:y,number:1,number:1,string:y|2,8/3/8/3;rep objs strict model.nope= ok -> string:y,string:y,object,object,string:y,number:1,number:1,string:y|2,8/3/8/3;rep objs n= Error: Cannot assign QString to int -> string:y,string:y,object,object,string:y,number:1,number:1,string:y|2,8/3/8/3;rep objs model.n= ok -> string:y,string:y,object,object,string:y,number:1,number:1,string:y|7,8/3/8/3;rep objs d.name= Error: Cannot assign to non-existent property "name" -> string:y,string:y,object,object,string:y,number:1,number:1,string:y|7,8/3/8/3;lv objs start -> string:b,string:b,object,object,string:b,number:1,number:1,string:b|2,3/3/3/3;lv objs name= ok -> string:z,string:z,object,object,string:z,number:1,number:1,string:z|2,4/3/4/3;lv objs model.name= ok -> string:m,string:m,object,object,string:m,number:1,number:1,string:m|2,5/3/5/3;lv objs modelData.name= ok -> string:s,string:s,object,object,string:s,number:1,number:1,string:s|2,6/3/6/3;lv objs model.modelData.name= ok -> string:t,string:t,object,object,string:t,number:1,number:1,string:t|2,7/3/7/3;lv objs modelData= TypeError: Cannot assign to read-only property "modelData" -> string:t,string:t,object,object,string:t,number:1,number:1,string:t|2,7/3/7/3;lv objs model.modelData= TypeError: Cannot assign to read-only property "modelData" -> string:t,string:t,object,object,string:t,number:1,number:1,string:t|2,7/3/7/3;lv objs index= TypeError: Cannot assign to read-only property "index" -> string:t,string:t,object,object,string:t,number:1,number:1,string:t|2,7/3/7/3;lv objs model.index= TypeError: Cannot assign to read-only property "index" -> string:t,string:t,object,object,string:t,number:1,number:1,string:t|2,7/3/7/3;lv objs model.nope= ok -> string:t,string:t,object,object,string:t,number:1,number:1,string:t|2,7/3/7/3;lv objs strict name= ok -> string:y,string:y,object,object,string:y,number:1,number:1,string:y|2,8/3/8/3;lv objs strict model.nope= ok -> string:y,string:y,object,object,string:y,number:1,number:1,string:y|2,8/3/8/3;lv objs n= Error: Cannot assign QString to int -> string:y,string:y,object,object,string:y,number:1,number:1,string:y|2,8/3/8/3;lv objs model.n= ok -> string:y,string:y,object,object,string:y,number:1,number:1,string:y|7,8/3/8/3;lv objs d.name= Error: Cannot assign to non-existent property "name" -> string:y,string:y,object,object,string:y,number:1,number:1,string:y|7,8/3/8/3;gv objs start -> string:b,string:b,object,object,string:b,number:1,number:1,string:b|2,3/3/3/3;gv objs name= ok -> string:z,string:z,object,object,string:z,number:1,number:1,string:z|2,4/3/4/3;gv objs model.name= ok -> string:m,string:m,object,object,string:m,number:1,number:1,string:m|2,5/3/5/3;gv objs modelData.name= ok -> string:s,string:s,object,object,string:s,number:1,number:1,string:s|2,6/3/6/3;gv objs model.modelData.name= ok -> string:t,string:t,object,object,string:t,number:1,number:1,string:t|2,7/3/7/3;gv objs modelData= TypeError: Cannot assign to read-only property "modelData" -> string:t,string:t,object,object,string:t,number:1,number:1,string:t|2,7/3/7/3;gv objs model.modelData= TypeError: Cannot assign to read-only property "modelData" -> string:t,string:t,object,object,string:t,number:1,number:1,string:t|2,7/3/7/3;gv objs index= TypeError: Cannot assign to read-only property "index" -> string:t,string:t,object,object,string:t,number:1,number:1,string:t|2,7/3/7/3;gv objs model.index= TypeError: Cannot assign to read-only property "index" -> string:t,string:t,object,object,string:t,number:1,number:1,string:t|2,7/3/7/3;gv objs model.nope= ok -> string:t,string:t,object,object,string:t,number:1,number:1,string:t|2,7/3/7/3;gv objs strict name= ok -> string:y,string:y,object,object,string:y,number:1,number:1,string:y|2,8/3/8/3;gv objs strict model.nope= ok -> string:y,string:y,object,object,string:y,number:1,number:1,string:y|2,8/3/8/3;gv objs n= Error: Cannot assign QString to int -> string:y,string:y,object,object,string:y,number:1,number:1,string:y|2,8/3/8/3;gv objs model.n= ok -> string:y,string:y,object,object,string:y,number:1,number:1,string:y|7,8/3/8/3;gv objs d.name= Error: Cannot assign to non-existent property "name" -> string:y,string:y,object,object,string:y,number:1,number:1,string:y|7,8/3/8/3;inst objs start -> string:b,string:b,object,object,string:b,number:1,number:1,string:b|2,3/3/3/3;inst objs name= ok -> string:z,string:z,object,object,string:z,number:1,number:1,string:z|2,4/3/4/3;inst objs model.name= ok -> string:m,string:m,object,object,string:m,number:1,number:1,string:m|2,5/3/5/3;inst objs modelData.name= ok -> string:s,string:s,object,object,string:s,number:1,number:1,string:s|2,6/3/6/3;inst objs model.modelData.name= ok -> string:t,string:t,object,object,string:t,number:1,number:1,string:t|2,7/3/7/3;inst objs modelData= TypeError: Cannot assign to read-only property "modelData" -> string:t,string:t,object,object,string:t,number:1,number:1,string:t|2,7/3/7/3;inst objs model.modelData= TypeError: Cannot assign to read-only property "modelData" -> string:t,string:t,object,object,string:t,number:1,number:1,string:t|2,7/3/7/3;inst objs index= TypeError: Cannot assign to read-only property "index" -> string:t,string:t,object,object,string:t,number:1,number:1,string:t|2,7/3/7/3;inst objs model.index= TypeError: Cannot assign to read-only property "index" -> string:t,string:t,object,object,string:t,number:1,number:1,string:t|2,7/3/7/3;inst objs model.nope= ok -> string:t,string:t,object,object,string:t,number:1,number:1,string:t|2,7/3/7/3;inst objs strict name= ok -> string:y,string:y,object,object,string:y,number:1,number:1,string:y|2,8/3/8/3;inst objs strict model.nope= ok -> string:y,string:y,object,object,string:y,number:1,number:1,string:y|2,8/3/8/3;inst objs n= Error: Cannot assign QString to int -> string:y,string:y,object,object,string:y,number:1,number:1,string:y|2,8/3/8/3;inst objs model.n= ok -> string:y,string:y,object,object,string:y,number:1,number:1,string:y|7,8/3/8/3;inst objs d.name= Error: Cannot assign to non-existent property "name" -> string:y,string:y,object,object,string:y,number:1,number:1,string:y|7,8/3/8/3
import QtQuick
import QtQml.Models
Item {
    id: root
    width: 400; height: 400
    property string result: ""
    property var seen: []
    property var cnt: ({})
    function bump(k) { cnt[k] = (cnt[k] || 0) + 1 }
    function counts(tag) { return [cnt[tag + " name"], cnt[tag + " md"], cnt[tag + " mname"], cnt[tag + " index"]].join("/") }
    function s(v) {
        if (v === undefined) return "undefined"
        if (v === null) return "null"
        if (typeof v === "object" || typeof v === "function") return typeof v
        return typeof v + ":" + v
    }
    function q(fn) { try { return s(fn()) } catch (e) { return ("" + e).replace(/ReferenceError: (\w+) is not defined/, "RefErr($1)").replace(/TypeError: Cannot read property '(\w+)' of undefined/, "NoProp($1)") } }
    ListModel { id: lm_rep; ListElement { name: "a"; n: 1 } ListElement { name: "b"; n: 2 } ListElement { name: "c"; n: 3 } }
    Repeater { id: rep_lm; model: lm_rep; delegate: Item {
        id: d
        property string tag: "rep lm"
        width: 10; height: 10
        property var bName: { root.bump("rep lm name"); try { return name } catch (e) { return "E" } }
        property var bMd: { root.bump("rep lm md"); try { return modelData } catch (e) { return "E" } }
        property var bModelName: { root.bump("rep lm mname"); try { return model.name } catch (e) { return "E" } }
        property var bIndex: { root.bump("rep lm index"); try { return index } catch (e) { return "E" } }
        function st() {
            return [root.q(() => name), root.q(() => model.name), root.q(() => modelData), root.q(() => model.modelData), root.q(() => modelData.name),
                root.q(() => index), root.q(() => model.index), root.q(() => lm_rep.get(1).name + "|" + lm_rep.get(1).n), root.counts("rep lm")].join(",")
        }
        function w(label, fn) { var e = "ok"; try { fn() } catch (x) { e = "" + x }; root.seen.push(tag + " " + label + " " + e + " -> " + st()) }
        function writes() {
            root.seen.push(tag + " start -> " + st())
            w("name=", () => { name = "z" })
            w("model.name=", () => { model.name = "m" })
            w("modelData.name=", () => { modelData.name = "s" })
            w("model.modelData.name=", () => { model.modelData.name = "t" })
            w("modelData=", () => { modelData = "q" })
            w("model.modelData=", () => { model.modelData = "r" })
            w("index=", () => { index = 5 })
            w("model.index=", () => { model.index = 6 })
            w("model.nope=", () => { model.nope = 1 })
            w("strict name=", () => { "use strict"; name = "y" })
            w("strict model.nope=", () => { "use strict"; model.nope = 1 })
            w("n=", () => { n = "str" })
            w("model.n=", () => { model.n = 7 })
            w("d.name=", () => { d.name = "o" })
        }
    } }
    ListModel { id: lm_lv; ListElement { name: "a"; n: 1 } ListElement { name: "b"; n: 2 } ListElement { name: "c"; n: 3 } }
    ListView { id: lv_lm; width: 100; height: 100; model: lm_lv; delegate: Item {
        id: d
        property string tag: "lv lm"
        width: 10; height: 10
        property var bName: { root.bump("lv lm name"); try { return name } catch (e) { return "E" } }
        property var bMd: { root.bump("lv lm md"); try { return modelData } catch (e) { return "E" } }
        property var bModelName: { root.bump("lv lm mname"); try { return model.name } catch (e) { return "E" } }
        property var bIndex: { root.bump("lv lm index"); try { return index } catch (e) { return "E" } }
        function st() {
            return [root.q(() => name), root.q(() => model.name), root.q(() => modelData), root.q(() => model.modelData), root.q(() => modelData.name),
                root.q(() => index), root.q(() => model.index), root.q(() => lm_lv.get(1).name + "|" + lm_lv.get(1).n), root.counts("lv lm")].join(",")
        }
        function w(label, fn) { var e = "ok"; try { fn() } catch (x) { e = "" + x }; root.seen.push(tag + " " + label + " " + e + " -> " + st()) }
        function writes() {
            root.seen.push(tag + " start -> " + st())
            w("name=", () => { name = "z" })
            w("model.name=", () => { model.name = "m" })
            w("modelData.name=", () => { modelData.name = "s" })
            w("model.modelData.name=", () => { model.modelData.name = "t" })
            w("modelData=", () => { modelData = "q" })
            w("model.modelData=", () => { model.modelData = "r" })
            w("index=", () => { index = 5 })
            w("model.index=", () => { model.index = 6 })
            w("model.nope=", () => { model.nope = 1 })
            w("strict name=", () => { "use strict"; name = "y" })
            w("strict model.nope=", () => { "use strict"; model.nope = 1 })
            w("n=", () => { n = "str" })
            w("model.n=", () => { model.n = 7 })
            w("d.name=", () => { d.name = "o" })
        }
    } }
    ListModel { id: lm_gv; ListElement { name: "a"; n: 1 } ListElement { name: "b"; n: 2 } ListElement { name: "c"; n: 3 } }
    GridView { id: gv_lm; width: 100; height: 100; cellWidth: 10; cellHeight: 10; model: lm_gv; delegate: Item {
        id: d
        property string tag: "gv lm"
        width: 10; height: 10
        property var bName: { root.bump("gv lm name"); try { return name } catch (e) { return "E" } }
        property var bMd: { root.bump("gv lm md"); try { return modelData } catch (e) { return "E" } }
        property var bModelName: { root.bump("gv lm mname"); try { return model.name } catch (e) { return "E" } }
        property var bIndex: { root.bump("gv lm index"); try { return index } catch (e) { return "E" } }
        function st() {
            return [root.q(() => name), root.q(() => model.name), root.q(() => modelData), root.q(() => model.modelData), root.q(() => modelData.name),
                root.q(() => index), root.q(() => model.index), root.q(() => lm_gv.get(1).name + "|" + lm_gv.get(1).n), root.counts("gv lm")].join(",")
        }
        function w(label, fn) { var e = "ok"; try { fn() } catch (x) { e = "" + x }; root.seen.push(tag + " " + label + " " + e + " -> " + st()) }
        function writes() {
            root.seen.push(tag + " start -> " + st())
            w("name=", () => { name = "z" })
            w("model.name=", () => { model.name = "m" })
            w("modelData.name=", () => { modelData.name = "s" })
            w("model.modelData.name=", () => { model.modelData.name = "t" })
            w("modelData=", () => { modelData = "q" })
            w("model.modelData=", () => { model.modelData = "r" })
            w("index=", () => { index = 5 })
            w("model.index=", () => { model.index = 6 })
            w("model.nope=", () => { model.nope = 1 })
            w("strict name=", () => { "use strict"; name = "y" })
            w("strict model.nope=", () => { "use strict"; model.nope = 1 })
            w("n=", () => { n = "str" })
            w("model.n=", () => { model.n = 7 })
            w("d.name=", () => { d.name = "o" })
        }
    } }
    ListModel { id: lm_inst; ListElement { name: "a"; n: 1 } ListElement { name: "b"; n: 2 } ListElement { name: "c"; n: 3 } }
    Instantiator { id: inst_lm; model: lm_inst; delegate: Item {
        id: d
        property string tag: "inst lm"
        width: 10; height: 10
        property var bName: { root.bump("inst lm name"); try { return name } catch (e) { return "E" } }
        property var bMd: { root.bump("inst lm md"); try { return modelData } catch (e) { return "E" } }
        property var bModelName: { root.bump("inst lm mname"); try { return model.name } catch (e) { return "E" } }
        property var bIndex: { root.bump("inst lm index"); try { return index } catch (e) { return "E" } }
        function st() {
            return [root.q(() => name), root.q(() => model.name), root.q(() => modelData), root.q(() => model.modelData), root.q(() => modelData.name),
                root.q(() => index), root.q(() => model.index), root.q(() => lm_inst.get(1).name + "|" + lm_inst.get(1).n), root.counts("inst lm")].join(",")
        }
        function w(label, fn) { var e = "ok"; try { fn() } catch (x) { e = "" + x }; root.seen.push(tag + " " + label + " " + e + " -> " + st()) }
        function writes() {
            root.seen.push(tag + " start -> " + st())
            w("name=", () => { name = "z" })
            w("model.name=", () => { model.name = "m" })
            w("modelData.name=", () => { modelData.name = "s" })
            w("model.modelData.name=", () => { model.modelData.name = "t" })
            w("modelData=", () => { modelData = "q" })
            w("model.modelData=", () => { model.modelData = "r" })
            w("index=", () => { index = 5 })
            w("model.index=", () => { model.index = 6 })
            w("model.nope=", () => { model.nope = 1 })
            w("strict name=", () => { "use strict"; name = "y" })
            w("strict model.nope=", () => { "use strict"; model.nope = 1 })
            w("n=", () => { n = "str" })
            w("model.n=", () => { model.n = 7 })
            w("d.name=", () => { d.name = "o" })
        }
    } }
    ListModel { id: lm1_rep; ListElement { name: "a" } ListElement { name: "b" } ListElement { name: "c" } }
    Repeater { id: rep_lm1; model: lm1_rep; delegate: Item {
        id: d
        property string tag: "rep lm1"
        width: 10; height: 10
        property var bName: { root.bump("rep lm1 name"); try { return name } catch (e) { return "E" } }
        property var bMd: { root.bump("rep lm1 md"); try { return modelData } catch (e) { return "E" } }
        property var bModelName: { root.bump("rep lm1 mname"); try { return model.name } catch (e) { return "E" } }
        property var bIndex: { root.bump("rep lm1 index"); try { return index } catch (e) { return "E" } }
        function st() {
            return [root.q(() => name), root.q(() => model.name), root.q(() => modelData), root.q(() => model.modelData), root.q(() => modelData.name),
                root.q(() => index), root.q(() => model.index), root.q(() => lm1_rep.get(1).name), root.counts("rep lm1")].join(",")
        }
        function w(label, fn) { var e = "ok"; try { fn() } catch (x) { e = "" + x }; root.seen.push(tag + " " + label + " " + e + " -> " + st()) }
        function writes() {
            root.seen.push(tag + " start -> " + st())
            w("name=", () => { name = "z" })
            w("model.name=", () => { model.name = "m" })
            w("modelData.name=", () => { modelData.name = "s" })
            w("model.modelData.name=", () => { model.modelData.name = "t" })
            w("modelData=", () => { modelData = "q" })
            w("model.modelData=", () => { model.modelData = "r" })
            w("index=", () => { index = 5 })
            w("model.index=", () => { model.index = 6 })
            w("model.nope=", () => { model.nope = 1 })
            w("strict name=", () => { "use strict"; name = "y" })
            w("strict model.nope=", () => { "use strict"; model.nope = 1 })
            w("n=", () => { n = "str" })
            w("model.n=", () => { model.n = 7 })
            w("d.name=", () => { d.name = "o" })
        }
    } }
    ListModel { id: lm1_lv; ListElement { name: "a" } ListElement { name: "b" } ListElement { name: "c" } }
    ListView { id: lv_lm1; width: 100; height: 100; model: lm1_lv; delegate: Item {
        id: d
        property string tag: "lv lm1"
        width: 10; height: 10
        property var bName: { root.bump("lv lm1 name"); try { return name } catch (e) { return "E" } }
        property var bMd: { root.bump("lv lm1 md"); try { return modelData } catch (e) { return "E" } }
        property var bModelName: { root.bump("lv lm1 mname"); try { return model.name } catch (e) { return "E" } }
        property var bIndex: { root.bump("lv lm1 index"); try { return index } catch (e) { return "E" } }
        function st() {
            return [root.q(() => name), root.q(() => model.name), root.q(() => modelData), root.q(() => model.modelData), root.q(() => modelData.name),
                root.q(() => index), root.q(() => model.index), root.q(() => lm1_lv.get(1).name), root.counts("lv lm1")].join(",")
        }
        function w(label, fn) { var e = "ok"; try { fn() } catch (x) { e = "" + x }; root.seen.push(tag + " " + label + " " + e + " -> " + st()) }
        function writes() {
            root.seen.push(tag + " start -> " + st())
            w("name=", () => { name = "z" })
            w("model.name=", () => { model.name = "m" })
            w("modelData.name=", () => { modelData.name = "s" })
            w("model.modelData.name=", () => { model.modelData.name = "t" })
            w("modelData=", () => { modelData = "q" })
            w("model.modelData=", () => { model.modelData = "r" })
            w("index=", () => { index = 5 })
            w("model.index=", () => { model.index = 6 })
            w("model.nope=", () => { model.nope = 1 })
            w("strict name=", () => { "use strict"; name = "y" })
            w("strict model.nope=", () => { "use strict"; model.nope = 1 })
            w("n=", () => { n = "str" })
            w("model.n=", () => { model.n = 7 })
            w("d.name=", () => { d.name = "o" })
        }
    } }
    ListModel { id: lm1_gv; ListElement { name: "a" } ListElement { name: "b" } ListElement { name: "c" } }
    GridView { id: gv_lm1; width: 100; height: 100; cellWidth: 10; cellHeight: 10; model: lm1_gv; delegate: Item {
        id: d
        property string tag: "gv lm1"
        width: 10; height: 10
        property var bName: { root.bump("gv lm1 name"); try { return name } catch (e) { return "E" } }
        property var bMd: { root.bump("gv lm1 md"); try { return modelData } catch (e) { return "E" } }
        property var bModelName: { root.bump("gv lm1 mname"); try { return model.name } catch (e) { return "E" } }
        property var bIndex: { root.bump("gv lm1 index"); try { return index } catch (e) { return "E" } }
        function st() {
            return [root.q(() => name), root.q(() => model.name), root.q(() => modelData), root.q(() => model.modelData), root.q(() => modelData.name),
                root.q(() => index), root.q(() => model.index), root.q(() => lm1_gv.get(1).name), root.counts("gv lm1")].join(",")
        }
        function w(label, fn) { var e = "ok"; try { fn() } catch (x) { e = "" + x }; root.seen.push(tag + " " + label + " " + e + " -> " + st()) }
        function writes() {
            root.seen.push(tag + " start -> " + st())
            w("name=", () => { name = "z" })
            w("model.name=", () => { model.name = "m" })
            w("modelData.name=", () => { modelData.name = "s" })
            w("model.modelData.name=", () => { model.modelData.name = "t" })
            w("modelData=", () => { modelData = "q" })
            w("model.modelData=", () => { model.modelData = "r" })
            w("index=", () => { index = 5 })
            w("model.index=", () => { model.index = 6 })
            w("model.nope=", () => { model.nope = 1 })
            w("strict name=", () => { "use strict"; name = "y" })
            w("strict model.nope=", () => { "use strict"; model.nope = 1 })
            w("n=", () => { n = "str" })
            w("model.n=", () => { model.n = 7 })
            w("d.name=", () => { d.name = "o" })
        }
    } }
    ListModel { id: lm1_inst; ListElement { name: "a" } ListElement { name: "b" } ListElement { name: "c" } }
    Instantiator { id: inst_lm1; model: lm1_inst; delegate: Item {
        id: d
        property string tag: "inst lm1"
        width: 10; height: 10
        property var bName: { root.bump("inst lm1 name"); try { return name } catch (e) { return "E" } }
        property var bMd: { root.bump("inst lm1 md"); try { return modelData } catch (e) { return "E" } }
        property var bModelName: { root.bump("inst lm1 mname"); try { return model.name } catch (e) { return "E" } }
        property var bIndex: { root.bump("inst lm1 index"); try { return index } catch (e) { return "E" } }
        function st() {
            return [root.q(() => name), root.q(() => model.name), root.q(() => modelData), root.q(() => model.modelData), root.q(() => modelData.name),
                root.q(() => index), root.q(() => model.index), root.q(() => lm1_inst.get(1).name), root.counts("inst lm1")].join(",")
        }
        function w(label, fn) { var e = "ok"; try { fn() } catch (x) { e = "" + x }; root.seen.push(tag + " " + label + " " + e + " -> " + st()) }
        function writes() {
            root.seen.push(tag + " start -> " + st())
            w("name=", () => { name = "z" })
            w("model.name=", () => { model.name = "m" })
            w("modelData.name=", () => { modelData.name = "s" })
            w("model.modelData.name=", () => { model.modelData.name = "t" })
            w("modelData=", () => { modelData = "q" })
            w("model.modelData=", () => { model.modelData = "r" })
            w("index=", () => { index = 5 })
            w("model.index=", () => { model.index = 6 })
            w("model.nope=", () => { model.nope = 1 })
            w("strict name=", () => { "use strict"; name = "y" })
            w("strict model.nope=", () => { "use strict"; model.nope = 1 })
            w("n=", () => { n = "str" })
            w("model.n=", () => { model.n = 7 })
            w("d.name=", () => { d.name = "o" })
        }
    } }
    Repeater { id: rep_num; model: 3; delegate: Item {
        id: d
        property string tag: "rep num"
        width: 10; height: 10
        property var bName: { root.bump("rep num name"); try { return name } catch (e) { return "E" } }
        property var bMd: { root.bump("rep num md"); try { return modelData } catch (e) { return "E" } }
        property var bModelName: { root.bump("rep num mname"); try { return model.name } catch (e) { return "E" } }
        property var bIndex: { root.bump("rep num index"); try { return index } catch (e) { return "E" } }
        function st() {
            return [root.q(() => name), root.q(() => model.name), root.q(() => modelData), root.q(() => model.modelData), root.q(() => modelData.name),
                root.q(() => index), root.q(() => model.index), root.q(() => "-"), root.counts("rep num")].join(",")
        }
        function w(label, fn) { var e = "ok"; try { fn() } catch (x) { e = "" + x }; root.seen.push(tag + " " + label + " " + e + " -> " + st()) }
        function writes() {
            root.seen.push(tag + " start -> " + st())
            w("name=", () => { name = "z" })
            w("model.name=", () => { model.name = "m" })
            w("modelData.name=", () => { modelData.name = "s" })
            w("model.modelData.name=", () => { model.modelData.name = "t" })
            w("modelData=", () => { modelData = "q" })
            w("model.modelData=", () => { model.modelData = "r" })
            w("index=", () => { index = 5 })
            w("model.index=", () => { model.index = 6 })
            w("model.nope=", () => { model.nope = 1 })
            w("strict name=", () => { "use strict"; name = "y" })
            w("strict model.nope=", () => { "use strict"; model.nope = 1 })
            w("n=", () => { n = "str" })
            w("model.n=", () => { model.n = 7 })
            w("d.name=", () => { d.name = "o" })
        }
    } }
    ListView { id: lv_num; width: 100; height: 100; model: 3; delegate: Item {
        id: d
        property string tag: "lv num"
        width: 10; height: 10
        property var bName: { root.bump("lv num name"); try { return name } catch (e) { return "E" } }
        property var bMd: { root.bump("lv num md"); try { return modelData } catch (e) { return "E" } }
        property var bModelName: { root.bump("lv num mname"); try { return model.name } catch (e) { return "E" } }
        property var bIndex: { root.bump("lv num index"); try { return index } catch (e) { return "E" } }
        function st() {
            return [root.q(() => name), root.q(() => model.name), root.q(() => modelData), root.q(() => model.modelData), root.q(() => modelData.name),
                root.q(() => index), root.q(() => model.index), root.q(() => "-"), root.counts("lv num")].join(",")
        }
        function w(label, fn) { var e = "ok"; try { fn() } catch (x) { e = "" + x }; root.seen.push(tag + " " + label + " " + e + " -> " + st()) }
        function writes() {
            root.seen.push(tag + " start -> " + st())
            w("name=", () => { name = "z" })
            w("model.name=", () => { model.name = "m" })
            w("modelData.name=", () => { modelData.name = "s" })
            w("model.modelData.name=", () => { model.modelData.name = "t" })
            w("modelData=", () => { modelData = "q" })
            w("model.modelData=", () => { model.modelData = "r" })
            w("index=", () => { index = 5 })
            w("model.index=", () => { model.index = 6 })
            w("model.nope=", () => { model.nope = 1 })
            w("strict name=", () => { "use strict"; name = "y" })
            w("strict model.nope=", () => { "use strict"; model.nope = 1 })
            w("n=", () => { n = "str" })
            w("model.n=", () => { model.n = 7 })
            w("d.name=", () => { d.name = "o" })
        }
    } }
    GridView { id: gv_num; width: 100; height: 100; cellWidth: 10; cellHeight: 10; model: 3; delegate: Item {
        id: d
        property string tag: "gv num"
        width: 10; height: 10
        property var bName: { root.bump("gv num name"); try { return name } catch (e) { return "E" } }
        property var bMd: { root.bump("gv num md"); try { return modelData } catch (e) { return "E" } }
        property var bModelName: { root.bump("gv num mname"); try { return model.name } catch (e) { return "E" } }
        property var bIndex: { root.bump("gv num index"); try { return index } catch (e) { return "E" } }
        function st() {
            return [root.q(() => name), root.q(() => model.name), root.q(() => modelData), root.q(() => model.modelData), root.q(() => modelData.name),
                root.q(() => index), root.q(() => model.index), root.q(() => "-"), root.counts("gv num")].join(",")
        }
        function w(label, fn) { var e = "ok"; try { fn() } catch (x) { e = "" + x }; root.seen.push(tag + " " + label + " " + e + " -> " + st()) }
        function writes() {
            root.seen.push(tag + " start -> " + st())
            w("name=", () => { name = "z" })
            w("model.name=", () => { model.name = "m" })
            w("modelData.name=", () => { modelData.name = "s" })
            w("model.modelData.name=", () => { model.modelData.name = "t" })
            w("modelData=", () => { modelData = "q" })
            w("model.modelData=", () => { model.modelData = "r" })
            w("index=", () => { index = 5 })
            w("model.index=", () => { model.index = 6 })
            w("model.nope=", () => { model.nope = 1 })
            w("strict name=", () => { "use strict"; name = "y" })
            w("strict model.nope=", () => { "use strict"; model.nope = 1 })
            w("n=", () => { n = "str" })
            w("model.n=", () => { model.n = 7 })
            w("d.name=", () => { d.name = "o" })
        }
    } }
    Instantiator { id: inst_num; model: 3; delegate: Item {
        id: d
        property string tag: "inst num"
        width: 10; height: 10
        property var bName: { root.bump("inst num name"); try { return name } catch (e) { return "E" } }
        property var bMd: { root.bump("inst num md"); try { return modelData } catch (e) { return "E" } }
        property var bModelName: { root.bump("inst num mname"); try { return model.name } catch (e) { return "E" } }
        property var bIndex: { root.bump("inst num index"); try { return index } catch (e) { return "E" } }
        function st() {
            return [root.q(() => name), root.q(() => model.name), root.q(() => modelData), root.q(() => model.modelData), root.q(() => modelData.name),
                root.q(() => index), root.q(() => model.index), root.q(() => "-"), root.counts("inst num")].join(",")
        }
        function w(label, fn) { var e = "ok"; try { fn() } catch (x) { e = "" + x }; root.seen.push(tag + " " + label + " " + e + " -> " + st()) }
        function writes() {
            root.seen.push(tag + " start -> " + st())
            w("name=", () => { name = "z" })
            w("model.name=", () => { model.name = "m" })
            w("modelData.name=", () => { modelData.name = "s" })
            w("model.modelData.name=", () => { model.modelData.name = "t" })
            w("modelData=", () => { modelData = "q" })
            w("model.modelData=", () => { model.modelData = "r" })
            w("index=", () => { index = 5 })
            w("model.index=", () => { model.index = 6 })
            w("model.nope=", () => { model.nope = 1 })
            w("strict name=", () => { "use strict"; name = "y" })
            w("strict model.nope=", () => { "use strict"; model.nope = 1 })
            w("n=", () => { n = "str" })
            w("model.n=", () => { model.n = 7 })
            w("d.name=", () => { d.name = "o" })
        }
    } }
    property var arr_rep: ["x", "y", "z"]
    Repeater { id: rep_arr; model: root.arr_rep; delegate: Item {
        id: d
        property string tag: "rep arr"
        width: 10; height: 10
        property var bName: { root.bump("rep arr name"); try { return name } catch (e) { return "E" } }
        property var bMd: { root.bump("rep arr md"); try { return modelData } catch (e) { return "E" } }
        property var bModelName: { root.bump("rep arr mname"); try { return model.name } catch (e) { return "E" } }
        property var bIndex: { root.bump("rep arr index"); try { return index } catch (e) { return "E" } }
        function st() {
            return [root.q(() => name), root.q(() => model.name), root.q(() => modelData), root.q(() => model.modelData), root.q(() => modelData.name),
                root.q(() => index), root.q(() => model.index), root.q(() => root.arr_rep[1]), root.counts("rep arr")].join(",")
        }
        function w(label, fn) { var e = "ok"; try { fn() } catch (x) { e = "" + x }; root.seen.push(tag + " " + label + " " + e + " -> " + st()) }
        function writes() {
            root.seen.push(tag + " start -> " + st())
            w("name=", () => { name = "z" })
            w("model.name=", () => { model.name = "m" })
            w("modelData.name=", () => { modelData.name = "s" })
            w("model.modelData.name=", () => { model.modelData.name = "t" })
            w("modelData=", () => { modelData = "q" })
            w("model.modelData=", () => { model.modelData = "r" })
            w("index=", () => { index = 5 })
            w("model.index=", () => { model.index = 6 })
            w("model.nope=", () => { model.nope = 1 })
            w("strict name=", () => { "use strict"; name = "y" })
            w("strict model.nope=", () => { "use strict"; model.nope = 1 })
            w("n=", () => { n = "str" })
            w("model.n=", () => { model.n = 7 })
            w("d.name=", () => { d.name = "o" })
        }
    } }
    property var arr_lv: ["x", "y", "z"]
    ListView { id: lv_arr; width: 100; height: 100; model: root.arr_lv; delegate: Item {
        id: d
        property string tag: "lv arr"
        width: 10; height: 10
        property var bName: { root.bump("lv arr name"); try { return name } catch (e) { return "E" } }
        property var bMd: { root.bump("lv arr md"); try { return modelData } catch (e) { return "E" } }
        property var bModelName: { root.bump("lv arr mname"); try { return model.name } catch (e) { return "E" } }
        property var bIndex: { root.bump("lv arr index"); try { return index } catch (e) { return "E" } }
        function st() {
            return [root.q(() => name), root.q(() => model.name), root.q(() => modelData), root.q(() => model.modelData), root.q(() => modelData.name),
                root.q(() => index), root.q(() => model.index), root.q(() => root.arr_lv[1]), root.counts("lv arr")].join(",")
        }
        function w(label, fn) { var e = "ok"; try { fn() } catch (x) { e = "" + x }; root.seen.push(tag + " " + label + " " + e + " -> " + st()) }
        function writes() {
            root.seen.push(tag + " start -> " + st())
            w("name=", () => { name = "z" })
            w("model.name=", () => { model.name = "m" })
            w("modelData.name=", () => { modelData.name = "s" })
            w("model.modelData.name=", () => { model.modelData.name = "t" })
            w("modelData=", () => { modelData = "q" })
            w("model.modelData=", () => { model.modelData = "r" })
            w("index=", () => { index = 5 })
            w("model.index=", () => { model.index = 6 })
            w("model.nope=", () => { model.nope = 1 })
            w("strict name=", () => { "use strict"; name = "y" })
            w("strict model.nope=", () => { "use strict"; model.nope = 1 })
            w("n=", () => { n = "str" })
            w("model.n=", () => { model.n = 7 })
            w("d.name=", () => { d.name = "o" })
        }
    } }
    property var arr_gv: ["x", "y", "z"]
    GridView { id: gv_arr; width: 100; height: 100; cellWidth: 10; cellHeight: 10; model: root.arr_gv; delegate: Item {
        id: d
        property string tag: "gv arr"
        width: 10; height: 10
        property var bName: { root.bump("gv arr name"); try { return name } catch (e) { return "E" } }
        property var bMd: { root.bump("gv arr md"); try { return modelData } catch (e) { return "E" } }
        property var bModelName: { root.bump("gv arr mname"); try { return model.name } catch (e) { return "E" } }
        property var bIndex: { root.bump("gv arr index"); try { return index } catch (e) { return "E" } }
        function st() {
            return [root.q(() => name), root.q(() => model.name), root.q(() => modelData), root.q(() => model.modelData), root.q(() => modelData.name),
                root.q(() => index), root.q(() => model.index), root.q(() => root.arr_gv[1]), root.counts("gv arr")].join(",")
        }
        function w(label, fn) { var e = "ok"; try { fn() } catch (x) { e = "" + x }; root.seen.push(tag + " " + label + " " + e + " -> " + st()) }
        function writes() {
            root.seen.push(tag + " start -> " + st())
            w("name=", () => { name = "z" })
            w("model.name=", () => { model.name = "m" })
            w("modelData.name=", () => { modelData.name = "s" })
            w("model.modelData.name=", () => { model.modelData.name = "t" })
            w("modelData=", () => { modelData = "q" })
            w("model.modelData=", () => { model.modelData = "r" })
            w("index=", () => { index = 5 })
            w("model.index=", () => { model.index = 6 })
            w("model.nope=", () => { model.nope = 1 })
            w("strict name=", () => { "use strict"; name = "y" })
            w("strict model.nope=", () => { "use strict"; model.nope = 1 })
            w("n=", () => { n = "str" })
            w("model.n=", () => { model.n = 7 })
            w("d.name=", () => { d.name = "o" })
        }
    } }
    property var arr_inst: ["x", "y", "z"]
    Instantiator { id: inst_arr; model: root.arr_inst; delegate: Item {
        id: d
        property string tag: "inst arr"
        width: 10; height: 10
        property var bName: { root.bump("inst arr name"); try { return name } catch (e) { return "E" } }
        property var bMd: { root.bump("inst arr md"); try { return modelData } catch (e) { return "E" } }
        property var bModelName: { root.bump("inst arr mname"); try { return model.name } catch (e) { return "E" } }
        property var bIndex: { root.bump("inst arr index"); try { return index } catch (e) { return "E" } }
        function st() {
            return [root.q(() => name), root.q(() => model.name), root.q(() => modelData), root.q(() => model.modelData), root.q(() => modelData.name),
                root.q(() => index), root.q(() => model.index), root.q(() => root.arr_inst[1]), root.counts("inst arr")].join(",")
        }
        function w(label, fn) { var e = "ok"; try { fn() } catch (x) { e = "" + x }; root.seen.push(tag + " " + label + " " + e + " -> " + st()) }
        function writes() {
            root.seen.push(tag + " start -> " + st())
            w("name=", () => { name = "z" })
            w("model.name=", () => { model.name = "m" })
            w("modelData.name=", () => { modelData.name = "s" })
            w("model.modelData.name=", () => { model.modelData.name = "t" })
            w("modelData=", () => { modelData = "q" })
            w("model.modelData=", () => { model.modelData = "r" })
            w("index=", () => { index = 5 })
            w("model.index=", () => { model.index = 6 })
            w("model.nope=", () => { model.nope = 1 })
            w("strict name=", () => { "use strict"; name = "y" })
            w("strict model.nope=", () => { "use strict"; model.nope = 1 })
            w("n=", () => { n = "str" })
            w("model.n=", () => { model.n = 7 })
            w("d.name=", () => { d.name = "o" })
        }
    } }
    property var arro_rep: [{ name: "a", n: 1 }, { name: "b", n: 2 }, { name: "c", n: 3 }]
    Repeater { id: rep_arro; model: root.arro_rep; delegate: Item {
        id: d
        property string tag: "rep arro"
        width: 10; height: 10
        property var bName: { root.bump("rep arro name"); try { return name } catch (e) { return "E" } }
        property var bMd: { root.bump("rep arro md"); try { return modelData } catch (e) { return "E" } }
        property var bModelName: { root.bump("rep arro mname"); try { return model.name } catch (e) { return "E" } }
        property var bIndex: { root.bump("rep arro index"); try { return index } catch (e) { return "E" } }
        function st() {
            return [root.q(() => name), root.q(() => model.name), root.q(() => modelData), root.q(() => model.modelData), root.q(() => modelData.name),
                root.q(() => index), root.q(() => model.index), root.q(() => root.arro_rep[1].name + "|" + root.arro_rep[1].n), root.counts("rep arro")].join(",")
        }
        function w(label, fn) { var e = "ok"; try { fn() } catch (x) { e = "" + x }; root.seen.push(tag + " " + label + " " + e + " -> " + st()) }
        function writes() {
            root.seen.push(tag + " start -> " + st())
            w("name=", () => { name = "z" })
            w("model.name=", () => { model.name = "m" })
            w("modelData=", () => { modelData = "q" })
            w("model.modelData=", () => { model.modelData = "r" })
            w("index=", () => { index = 5 })
            w("model.index=", () => { model.index = 6 })
            w("model.nope=", () => { model.nope = 1 })
            w("strict name=", () => { "use strict"; name = "y" })
            w("strict model.nope=", () => { "use strict"; model.nope = 1 })
            w("n=", () => { n = "str" })
            w("model.n=", () => { model.n = 7 })
            w("d.name=", () => { d.name = "o" })
        }
    } }
    property var arro_lv: [{ name: "a", n: 1 }, { name: "b", n: 2 }, { name: "c", n: 3 }]
    ListView { id: lv_arro; width: 100; height: 100; model: root.arro_lv; delegate: Item {
        id: d
        property string tag: "lv arro"
        width: 10; height: 10
        property var bName: { root.bump("lv arro name"); try { return name } catch (e) { return "E" } }
        property var bMd: { root.bump("lv arro md"); try { return modelData } catch (e) { return "E" } }
        property var bModelName: { root.bump("lv arro mname"); try { return model.name } catch (e) { return "E" } }
        property var bIndex: { root.bump("lv arro index"); try { return index } catch (e) { return "E" } }
        function st() {
            return [root.q(() => name), root.q(() => model.name), root.q(() => modelData), root.q(() => model.modelData), root.q(() => modelData.name),
                root.q(() => index), root.q(() => model.index), root.q(() => root.arro_lv[1].name + "|" + root.arro_lv[1].n), root.counts("lv arro")].join(",")
        }
        function w(label, fn) { var e = "ok"; try { fn() } catch (x) { e = "" + x }; root.seen.push(tag + " " + label + " " + e + " -> " + st()) }
        function writes() {
            root.seen.push(tag + " start -> " + st())
            w("name=", () => { name = "z" })
            w("model.name=", () => { model.name = "m" })
            w("modelData=", () => { modelData = "q" })
            w("model.modelData=", () => { model.modelData = "r" })
            w("index=", () => { index = 5 })
            w("model.index=", () => { model.index = 6 })
            w("model.nope=", () => { model.nope = 1 })
            w("strict name=", () => { "use strict"; name = "y" })
            w("strict model.nope=", () => { "use strict"; model.nope = 1 })
            w("n=", () => { n = "str" })
            w("model.n=", () => { model.n = 7 })
            w("d.name=", () => { d.name = "o" })
        }
    } }
    property var arro_gv: [{ name: "a", n: 1 }, { name: "b", n: 2 }, { name: "c", n: 3 }]
    GridView { id: gv_arro; width: 100; height: 100; cellWidth: 10; cellHeight: 10; model: root.arro_gv; delegate: Item {
        id: d
        property string tag: "gv arro"
        width: 10; height: 10
        property var bName: { root.bump("gv arro name"); try { return name } catch (e) { return "E" } }
        property var bMd: { root.bump("gv arro md"); try { return modelData } catch (e) { return "E" } }
        property var bModelName: { root.bump("gv arro mname"); try { return model.name } catch (e) { return "E" } }
        property var bIndex: { root.bump("gv arro index"); try { return index } catch (e) { return "E" } }
        function st() {
            return [root.q(() => name), root.q(() => model.name), root.q(() => modelData), root.q(() => model.modelData), root.q(() => modelData.name),
                root.q(() => index), root.q(() => model.index), root.q(() => root.arro_gv[1].name + "|" + root.arro_gv[1].n), root.counts("gv arro")].join(",")
        }
        function w(label, fn) { var e = "ok"; try { fn() } catch (x) { e = "" + x }; root.seen.push(tag + " " + label + " " + e + " -> " + st()) }
        function writes() {
            root.seen.push(tag + " start -> " + st())
            w("name=", () => { name = "z" })
            w("model.name=", () => { model.name = "m" })
            w("modelData=", () => { modelData = "q" })
            w("model.modelData=", () => { model.modelData = "r" })
            w("index=", () => { index = 5 })
            w("model.index=", () => { model.index = 6 })
            w("model.nope=", () => { model.nope = 1 })
            w("strict name=", () => { "use strict"; name = "y" })
            w("strict model.nope=", () => { "use strict"; model.nope = 1 })
            w("n=", () => { n = "str" })
            w("model.n=", () => { model.n = 7 })
            w("d.name=", () => { d.name = "o" })
        }
    } }
    property var arro_inst: [{ name: "a", n: 1 }, { name: "b", n: 2 }, { name: "c", n: 3 }]
    Instantiator { id: inst_arro; model: root.arro_inst; delegate: Item {
        id: d
        property string tag: "inst arro"
        width: 10; height: 10
        property var bName: { root.bump("inst arro name"); try { return name } catch (e) { return "E" } }
        property var bMd: { root.bump("inst arro md"); try { return modelData } catch (e) { return "E" } }
        property var bModelName: { root.bump("inst arro mname"); try { return model.name } catch (e) { return "E" } }
        property var bIndex: { root.bump("inst arro index"); try { return index } catch (e) { return "E" } }
        function st() {
            return [root.q(() => name), root.q(() => model.name), root.q(() => modelData), root.q(() => model.modelData), root.q(() => modelData.name),
                root.q(() => index), root.q(() => model.index), root.q(() => root.arro_inst[1].name + "|" + root.arro_inst[1].n), root.counts("inst arro")].join(",")
        }
        function w(label, fn) { var e = "ok"; try { fn() } catch (x) { e = "" + x }; root.seen.push(tag + " " + label + " " + e + " -> " + st()) }
        function writes() {
            root.seen.push(tag + " start -> " + st())
            w("name=", () => { name = "z" })
            w("model.name=", () => { model.name = "m" })
            w("modelData=", () => { modelData = "q" })
            w("model.modelData=", () => { model.modelData = "r" })
            w("index=", () => { index = 5 })
            w("model.index=", () => { model.index = 6 })
            w("model.nope=", () => { model.nope = 1 })
            w("strict name=", () => { "use strict"; name = "y" })
            w("strict model.nope=", () => { "use strict"; model.nope = 1 })
            w("n=", () => { n = "str" })
            w("model.n=", () => { model.n = 7 })
            w("d.name=", () => { d.name = "o" })
        }
    } }
    property list<QtObject> objs_rep: [ QtObject { property string name: "a"; property int n: 1 }, QtObject { property string name: "b"; property int n: 2 }, QtObject { property string name: "c"; property int n: 3 } ]
    Repeater { id: rep_objs; model: root.objs_rep; delegate: Item {
        id: d
        property string tag: "rep objs"
        width: 10; height: 10
        property var bName: { root.bump("rep objs name"); try { return name } catch (e) { return "E" } }
        property var bMd: { root.bump("rep objs md"); try { return modelData } catch (e) { return "E" } }
        property var bModelName: { root.bump("rep objs mname"); try { return model.name } catch (e) { return "E" } }
        property var bIndex: { root.bump("rep objs index"); try { return index } catch (e) { return "E" } }
        function st() {
            return [root.q(() => name), root.q(() => model.name), root.q(() => modelData), root.q(() => model.modelData), root.q(() => modelData.name),
                root.q(() => index), root.q(() => model.index), root.q(() => root.objs_rep[1].name + "|" + root.objs_rep[1].n), root.counts("rep objs")].join(",")
        }
        function w(label, fn) { var e = "ok"; try { fn() } catch (x) { e = "" + x }; root.seen.push(tag + " " + label + " " + e + " -> " + st()) }
        function writes() {
            root.seen.push(tag + " start -> " + st())
            w("name=", () => { name = "z" })
            w("model.name=", () => { model.name = "m" })
            w("modelData.name=", () => { modelData.name = "s" })
            w("model.modelData.name=", () => { model.modelData.name = "t" })
            w("modelData=", () => { modelData = "q" })
            w("model.modelData=", () => { model.modelData = "r" })
            w("index=", () => { index = 5 })
            w("model.index=", () => { model.index = 6 })
            w("model.nope=", () => { model.nope = 1 })
            w("strict name=", () => { "use strict"; name = "y" })
            w("strict model.nope=", () => { "use strict"; model.nope = 1 })
            w("n=", () => { n = "str" })
            w("model.n=", () => { model.n = 7 })
            w("d.name=", () => { d.name = "o" })
        }
    } }
    property list<QtObject> objs_lv: [ QtObject { property string name: "a"; property int n: 1 }, QtObject { property string name: "b"; property int n: 2 }, QtObject { property string name: "c"; property int n: 3 } ]
    ListView { id: lv_objs; width: 100; height: 100; model: root.objs_lv; delegate: Item {
        id: d
        property string tag: "lv objs"
        width: 10; height: 10
        property var bName: { root.bump("lv objs name"); try { return name } catch (e) { return "E" } }
        property var bMd: { root.bump("lv objs md"); try { return modelData } catch (e) { return "E" } }
        property var bModelName: { root.bump("lv objs mname"); try { return model.name } catch (e) { return "E" } }
        property var bIndex: { root.bump("lv objs index"); try { return index } catch (e) { return "E" } }
        function st() {
            return [root.q(() => name), root.q(() => model.name), root.q(() => modelData), root.q(() => model.modelData), root.q(() => modelData.name),
                root.q(() => index), root.q(() => model.index), root.q(() => root.objs_lv[1].name + "|" + root.objs_lv[1].n), root.counts("lv objs")].join(",")
        }
        function w(label, fn) { var e = "ok"; try { fn() } catch (x) { e = "" + x }; root.seen.push(tag + " " + label + " " + e + " -> " + st()) }
        function writes() {
            root.seen.push(tag + " start -> " + st())
            w("name=", () => { name = "z" })
            w("model.name=", () => { model.name = "m" })
            w("modelData.name=", () => { modelData.name = "s" })
            w("model.modelData.name=", () => { model.modelData.name = "t" })
            w("modelData=", () => { modelData = "q" })
            w("model.modelData=", () => { model.modelData = "r" })
            w("index=", () => { index = 5 })
            w("model.index=", () => { model.index = 6 })
            w("model.nope=", () => { model.nope = 1 })
            w("strict name=", () => { "use strict"; name = "y" })
            w("strict model.nope=", () => { "use strict"; model.nope = 1 })
            w("n=", () => { n = "str" })
            w("model.n=", () => { model.n = 7 })
            w("d.name=", () => { d.name = "o" })
        }
    } }
    property list<QtObject> objs_gv: [ QtObject { property string name: "a"; property int n: 1 }, QtObject { property string name: "b"; property int n: 2 }, QtObject { property string name: "c"; property int n: 3 } ]
    GridView { id: gv_objs; width: 100; height: 100; cellWidth: 10; cellHeight: 10; model: root.objs_gv; delegate: Item {
        id: d
        property string tag: "gv objs"
        width: 10; height: 10
        property var bName: { root.bump("gv objs name"); try { return name } catch (e) { return "E" } }
        property var bMd: { root.bump("gv objs md"); try { return modelData } catch (e) { return "E" } }
        property var bModelName: { root.bump("gv objs mname"); try { return model.name } catch (e) { return "E" } }
        property var bIndex: { root.bump("gv objs index"); try { return index } catch (e) { return "E" } }
        function st() {
            return [root.q(() => name), root.q(() => model.name), root.q(() => modelData), root.q(() => model.modelData), root.q(() => modelData.name),
                root.q(() => index), root.q(() => model.index), root.q(() => root.objs_gv[1].name + "|" + root.objs_gv[1].n), root.counts("gv objs")].join(",")
        }
        function w(label, fn) { var e = "ok"; try { fn() } catch (x) { e = "" + x }; root.seen.push(tag + " " + label + " " + e + " -> " + st()) }
        function writes() {
            root.seen.push(tag + " start -> " + st())
            w("name=", () => { name = "z" })
            w("model.name=", () => { model.name = "m" })
            w("modelData.name=", () => { modelData.name = "s" })
            w("model.modelData.name=", () => { model.modelData.name = "t" })
            w("modelData=", () => { modelData = "q" })
            w("model.modelData=", () => { model.modelData = "r" })
            w("index=", () => { index = 5 })
            w("model.index=", () => { model.index = 6 })
            w("model.nope=", () => { model.nope = 1 })
            w("strict name=", () => { "use strict"; name = "y" })
            w("strict model.nope=", () => { "use strict"; model.nope = 1 })
            w("n=", () => { n = "str" })
            w("model.n=", () => { model.n = 7 })
            w("d.name=", () => { d.name = "o" })
        }
    } }
    property list<QtObject> objs_inst: [ QtObject { property string name: "a"; property int n: 1 }, QtObject { property string name: "b"; property int n: 2 }, QtObject { property string name: "c"; property int n: 3 } ]
    Instantiator { id: inst_objs; model: root.objs_inst; delegate: Item {
        id: d
        property string tag: "inst objs"
        width: 10; height: 10
        property var bName: { root.bump("inst objs name"); try { return name } catch (e) { return "E" } }
        property var bMd: { root.bump("inst objs md"); try { return modelData } catch (e) { return "E" } }
        property var bModelName: { root.bump("inst objs mname"); try { return model.name } catch (e) { return "E" } }
        property var bIndex: { root.bump("inst objs index"); try { return index } catch (e) { return "E" } }
        function st() {
            return [root.q(() => name), root.q(() => model.name), root.q(() => modelData), root.q(() => model.modelData), root.q(() => modelData.name),
                root.q(() => index), root.q(() => model.index), root.q(() => root.objs_inst[1].name + "|" + root.objs_inst[1].n), root.counts("inst objs")].join(",")
        }
        function w(label, fn) { var e = "ok"; try { fn() } catch (x) { e = "" + x }; root.seen.push(tag + " " + label + " " + e + " -> " + st()) }
        function writes() {
            root.seen.push(tag + " start -> " + st())
            w("name=", () => { name = "z" })
            w("model.name=", () => { model.name = "m" })
            w("modelData.name=", () => { modelData.name = "s" })
            w("model.modelData.name=", () => { model.modelData.name = "t" })
            w("modelData=", () => { modelData = "q" })
            w("model.modelData=", () => { model.modelData = "r" })
            w("index=", () => { index = 5 })
            w("model.index=", () => { model.index = 6 })
            w("model.nope=", () => { model.nope = 1 })
            w("strict name=", () => { "use strict"; name = "y" })
            w("strict model.nope=", () => { "use strict"; model.nope = 1 })
            w("n=", () => { n = "str" })
            w("model.n=", () => { model.n = 7 })
            w("d.name=", () => { d.name = "o" })
        }
    } }
    function run() {
        try { rep_lm.itemAt(1).writes() } catch (e) { seen.push("rep lm FAILED " + e) }
        try { lv_lm.itemAtIndex(1).writes() } catch (e) { seen.push("lv lm FAILED " + e) }
        try { gv_lm.itemAtIndex(1).writes() } catch (e) { seen.push("gv lm FAILED " + e) }
        try { inst_lm.objectAt(1).writes() } catch (e) { seen.push("inst lm FAILED " + e) }
        try { rep_lm1.itemAt(1).writes() } catch (e) { seen.push("rep lm1 FAILED " + e) }
        try { lv_lm1.itemAtIndex(1).writes() } catch (e) { seen.push("lv lm1 FAILED " + e) }
        try { gv_lm1.itemAtIndex(1).writes() } catch (e) { seen.push("gv lm1 FAILED " + e) }
        try { inst_lm1.objectAt(1).writes() } catch (e) { seen.push("inst lm1 FAILED " + e) }
        try { rep_num.itemAt(1).writes() } catch (e) { seen.push("rep num FAILED " + e) }
        try { lv_num.itemAtIndex(1).writes() } catch (e) { seen.push("lv num FAILED " + e) }
        try { gv_num.itemAtIndex(1).writes() } catch (e) { seen.push("gv num FAILED " + e) }
        try { inst_num.objectAt(1).writes() } catch (e) { seen.push("inst num FAILED " + e) }
        try { rep_arr.itemAt(1).writes() } catch (e) { seen.push("rep arr FAILED " + e) }
        try { lv_arr.itemAtIndex(1).writes() } catch (e) { seen.push("lv arr FAILED " + e) }
        try { gv_arr.itemAtIndex(1).writes() } catch (e) { seen.push("gv arr FAILED " + e) }
        try { inst_arr.objectAt(1).writes() } catch (e) { seen.push("inst arr FAILED " + e) }
        try { rep_arro.itemAt(1).writes() } catch (e) { seen.push("rep arro FAILED " + e) }
        try { lv_arro.itemAtIndex(1).writes() } catch (e) { seen.push("lv arro FAILED " + e) }
        try { gv_arro.itemAtIndex(1).writes() } catch (e) { seen.push("gv arro FAILED " + e) }
        try { inst_arro.objectAt(1).writes() } catch (e) { seen.push("inst arro FAILED " + e) }
        try { rep_objs.itemAt(1).writes() } catch (e) { seen.push("rep objs FAILED " + e) }
        try { lv_objs.itemAtIndex(1).writes() } catch (e) { seen.push("lv objs FAILED " + e) }
        try { gv_objs.itemAtIndex(1).writes() } catch (e) { seen.push("gv objs FAILED " + e) }
        try { inst_objs.objectAt(1).writes() } catch (e) { seen.push("inst objs FAILED " + e) }
        result = seen.join(";")
    }
    Component.onCompleted: Qt.callLater(run)
}
